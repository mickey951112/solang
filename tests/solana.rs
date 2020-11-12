//
extern crate byteorder;
extern crate ethabi;
extern crate ethereum_types;
extern crate libc;
extern crate solana_rbpf;
extern crate solang;

mod solana_helpers;

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use ethabi::Token;
use libc::c_char;
use solana_helpers::allocator_bump::BPFAllocator;
use solana_rbpf::{
    error::EbpfError,
    memory_region::{translate_addr, MemoryRegion},
    user_error::UserError,
    vm::{Config, EbpfVm, SyscallObject},
};
use solang::{compile, file_cache::FileCache, sema::diagnostics, Target};
use std::alloc::Layout;
use std::io::Write;
use std::mem::{align_of, size_of};

fn build_solidity(src: &'static str) -> VM {
    let mut cache = FileCache::new();

    cache.set_file_contents("test.sol".to_string(), src.to_string());

    let (res, ns) = compile(
        "test.sol",
        &mut cache,
        inkwell::OptimizationLevel::Default,
        Target::Solana,
    );

    diagnostics::print_messages(&mut cache, &ns, false);

    for v in &res {
        println!("contract size:{}", v.0.len());
    }

    assert_eq!(res.is_empty(), false);

    // resolve
    let (code, abi) = res.last().unwrap().clone();

    VM {
        code,
        abi: ethabi::Contract::load(abi.as_bytes()).unwrap(),
        printbuf: String::new(),
        output: Vec::new(),
        data: Vec::new(),
    }
}

const MAX_PERMITTED_DATA_INCREASE: usize = 10 * 1024;

fn serialize_parameters(input: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();

    // ka_num
    v.write_u64::<LittleEndian>(2).unwrap();
    for account_no in 0..2 {
        // dup_info
        v.write_u8(0xff).unwrap();
        // signer
        v.write_u8(1).unwrap();
        // is_writable
        v.write_u8(1).unwrap();
        // executable
        v.write_u8(1).unwrap();
        // padding
        v.write_all(&[0u8; 4]).unwrap();
        // key
        v.write_all(&[0u8; 32]).unwrap();
        // owner
        v.write_all(&[0u8; 32]).unwrap();
        // lamports
        v.write_u64::<LittleEndian>(0).unwrap();

        // account data
        // data len
        if account_no == 1 {
            v.write_u64::<LittleEndian>(1024).unwrap();
            let mut data = data.to_vec();
            data.resize(1024, 0);
            v.write_all(&data).unwrap();
        } else {
            v.write_u64::<LittleEndian>(1024).unwrap();
            v.write_all(&[0u8; 1024]).unwrap();
        }
        v.write_all(&[0u8; MAX_PERMITTED_DATA_INCREASE]).unwrap();

        let padding = v.len() % 8;
        if padding != 0 {
            let mut p = Vec::new();
            p.resize(8 - padding, 0);
            v.extend_from_slice(&p);
        }
        // rent epoch
        v.write_u64::<LittleEndian>(0).unwrap();
    }

    // calldata
    v.write_u64::<LittleEndian>(input.len() as u64).unwrap();
    v.write_all(input).unwrap();

    // program id
    v.write_all(&[0u8; 32]).unwrap();

    v
}

// We want to extract the account data
fn deserialize_parameters(input: &[u8]) -> Vec<Vec<u8>> {
    let mut start = 0;

    let ka_num = LittleEndian::read_u64(&input[start..]);
    start += size_of::<u64>();

    let mut res = Vec::new();

    for _ in 0..ka_num {
        start += 8 + 32 + 32 + 8;

        let data_len = LittleEndian::read_u64(&input[start..]) as usize;
        start += size_of::<u64>();

        res.push(input[start..start + data_len].to_vec());

        start += data_len + MAX_PERMITTED_DATA_INCREASE;

        let padding = start % 8;
        if padding > 0 {
            start += 8 - padding
        }

        start += size_of::<u64>();
    }

    res
}

struct VM {
    code: Vec<u8>,
    abi: ethabi::Contract,
    printbuf: String,
    data: Vec<u8>,
    output: Vec<u8>,
}

struct Printer<'a> {
    buf: &'a mut String,
}

impl<'a> SyscallObject<UserError> for Printer<'a> {
    fn call(
        &mut self,
        vm_addr: u64,
        len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        ro_regions: &[MemoryRegion],
        _rw_regions: &[MemoryRegion],
    ) -> Result<u64, EbpfError<UserError>> {
        let host_addr = translate_addr(vm_addr, len as usize, "Load", 0, ro_regions)?;
        let c_buf: *const c_char = host_addr as *const c_char;
        unsafe {
            for i in 0..len {
                let c = std::ptr::read(c_buf.offset(i as isize));
                if c == 0 {
                    break;
                }
            }
            let message = std::str::from_utf8(std::slice::from_raw_parts(
                host_addr as *const u8,
                len as usize,
            ))
            .unwrap();
            println!("log: {}", message);
            self.buf.push_str(message);
            Ok(0)
        }
    }
}

// Shamelessly stolen from solana source

/// Dynamic memory allocation syscall called when the BPF program calls
/// `sol_alloc_free_()`.  The allocator is expected to allocate/free
/// from/to a given chunk of memory and enforce size restrictions.  The
/// memory chunk is given to the allocator during allocator creation and
/// information about that memory (start address and size) is passed
/// to the VM to use for enforcement.
pub struct SyscallAllocFree {
    allocator: BPFAllocator,
}

const DEFAULT_HEAP_SIZE: usize = 32 * 1024;
pub const MM_HEAP_START: u64 = 0x300000000;
/// Start of the input buffers in the memory map

impl SyscallObject<UserError> for SyscallAllocFree {
    fn call(
        &mut self,
        size: u64,
        free_addr: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _ro_regions: &[MemoryRegion],
        _rw_regions: &[MemoryRegion],
    ) -> Result<u64, EbpfError<UserError>> {
        let align = align_of::<u128>();
        let layout = match Layout::from_size_align(size as usize, align) {
            Ok(layout) => layout,
            Err(_) => return Ok(0),
        };
        if free_addr == 0 {
            Ok(self.allocator.alloc(layout))
        } else {
            self.allocator.dealloc(free_addr, layout);
            Ok(0)
        }
    }
}

impl VM {
    fn execute(&mut self, buf: &mut String, calldata: &[u8]) {
        println!("running bpf with calldata:{}", hex::encode(calldata));

        let executable =
            EbpfVm::<UserError>::create_executable_from_elf(&self.code, None).expect("should work");
        let mut vm = EbpfVm::<UserError>::new(executable.as_ref(), Config::default()).unwrap();

        vm.register_syscall_with_context_ex("sol_log_", Box::new(Printer { buf }))
            .unwrap();

        let heap = vec![0_u8; DEFAULT_HEAP_SIZE];
        let heap_region = MemoryRegion::new_from_slice(&heap, MM_HEAP_START);
        vm.register_syscall_with_context_ex(
            "sol_alloc_free_",
            Box::new(SyscallAllocFree {
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            }),
        )
        .unwrap();

        let parameter_bytes = serialize_parameters(&calldata, &self.data);

        let res = vm
            .execute_program(&parameter_bytes, &[], &[heap_region])
            .unwrap();

        let mut accounts = deserialize_parameters(&parameter_bytes);

        let output = accounts.remove(0);
        let data = accounts.remove(0);

        println!(
            "output: {} \ndata: {}",
            hex::encode(&output),
            hex::encode(&data)
        );

        let len = LittleEndian::read_u64(&output);
        self.output = output[8..len as usize + 8].to_vec();
        self.data = data;

        println!("account: {}", hex::encode(&self.output));

        assert_eq!(res, 0);
    }

    fn constructor(&mut self, args: &[Token]) {
        let calldata = if let Some(constructor) = &self.abi.constructor {
            constructor.encode_input(Vec::new(), args).unwrap()
        } else {
            Vec::new()
        };

        let mut buf = String::new();
        self.execute(&mut buf, &calldata);
        self.printbuf = buf;
    }

    fn function(&mut self, name: &str, args: &[Token]) -> Vec<Token> {
        let calldata = match self.abi.functions[name][0].encode_input(args) {
            Ok(n) => n,
            Err(x) => panic!(format!("{}", x)),
        };

        let mut buf = String::new();
        self.execute(&mut buf, &calldata);
        self.printbuf = buf;

        self.abi.functions[name][0]
            .decode_output(&self.output)
            .unwrap()
    }
}

#[test]
fn simple() {
    let mut vm = build_solidity(
        r#"
        contract foo {
            constructor() {
                print("Hello from constructor");
            }

            function test() public {
                print("Hello from function");
            }
        }"#,
    );

    vm.constructor(&[]);

    assert_eq!(vm.printbuf, "Hello from constructor");

    vm.printbuf = String::new();

    vm.function("test", &[]);

    assert_eq!(vm.printbuf, "Hello from function");
}

#[test]
fn parameters() {
    let mut vm = build_solidity(
        r#"
        contract foo {
            function test(uint32 x, uint64 y) public {
                if (x == 10) {
                    print("x is 10");
                }

                if (y == 102) {
                    print("y is 102");
                }
            }
        }"#,
    );

    vm.constructor(&[]);

    vm.function(
        "test",
        &[
            ethabi::Token::Uint(ethereum_types::U256::from(10)),
            ethabi::Token::Uint(ethereum_types::U256::from(10)),
        ],
    );

    assert_eq!(vm.printbuf, "x is 10");

    vm.function(
        "test",
        &[
            ethabi::Token::Uint(ethereum_types::U256::from(99)),
            ethabi::Token::Uint(ethereum_types::U256::from(102)),
        ],
    );

    assert_eq!(vm.printbuf, "y is 102");
}

#[test]
fn returns() {
    let mut vm = build_solidity(
        r#"
        contract foo {
            function test(uint32 x) public returns (uint32) {
                return x * x;
            }
        }"#,
    );

    vm.constructor(&[]);

    let returns = vm.function(
        "test",
        &[ethabi::Token::Uint(ethereum_types::U256::from(10))],
    );

    assert_eq!(
        returns,
        vec![ethabi::Token::Uint(ethereum_types::U256::from(100))]
    );

    let mut vm = build_solidity(
        r#"
        contract foo {
            function test(uint64 x) public returns (bool, uint64) {
                return (true, x * 961748941);
            }
        }"#,
    );

    vm.constructor(&[]);

    let returns = vm.function(
        "test",
        &[ethabi::Token::Uint(ethereum_types::U256::from(982451653))],
    );

    assert_eq!(
        returns,
        vec![
            ethabi::Token::Bool(true),
            ethabi::Token::Uint(ethereum_types::U256::from(961748941u64 * 982451653u64))
        ]
    );
}

#[test]
fn flipper() {
    let mut vm = build_solidity(
        r#"
        contract flipper {
            bool private value;

            /// Constructor that initializes the `bool` value to the given `init_value`.
            constructor(bool initvalue) {
                value = initvalue;
            }

            /// A message that can be called on instantiated contracts.
            /// This one flips the value of the stored `bool` from `true`
            /// to `false` and vice versa.
            function flip() public {
                value = !value;
            }

            /// Simply returns the current value of our `bool`.
            function get() public view returns (bool) {
                return value;
            }
        }"#,
    );

    vm.constructor(&[ethabi::Token::Bool(true)]);

    assert_eq!(
        vm.data[0..9].to_vec(),
        hex::decode("6fc90ec5ae05628b01").unwrap()
    );

    let returns = vm.function("get", &[]);

    assert_eq!(returns, vec![ethabi::Token::Bool(true)]);

    vm.function("flip", &[]);

    assert_eq!(
        vm.data[0..9].to_vec(),
        hex::decode("6fc90ec5ae05628b00").unwrap()
    );

    let returns = vm.function("get", &[]);

    assert_eq!(returns, vec![ethabi::Token::Bool(false)]);
}
