
use parity_scale_codec::Encode;
use parity_scale_codec_derive::{Encode, Decode};

use super::build_solidity;

#[test]
fn simple_solidiy_compile_and_run() {
    #[derive(Debug, PartialEq, Encode, Decode)]
    struct FooReturn {
        value: u32
    }

    // parse
    let (runtime, mut store) = build_solidity("
        contract test {
            function foo() public returns (uint32) {
                return 2;
            }
        }",
    );

    runtime.function(&mut store, "foo", Vec::new());

    let ret = FooReturn{ value: 2 };

    assert_eq!(store.scratch, ret.encode());
}

#[test]
fn flipper() {
    // parse
    let (runtime, mut store) = build_solidity("
        contract flipper {
            bool private value;

            constructor(bool initvalue) public {
                value = initvalue;
            }

            function flip() public {
                value = !value;
            }

            function get() public view returns (bool) {
                return value;
            }
        }
        ",
    );

    #[derive(Debug, PartialEq, Encode, Decode)]
    struct GetReturn(bool);

    runtime.function(&mut store, "get", Vec::new());

    assert_eq!(store.scratch, GetReturn(false).encode());

    runtime.function(&mut store, "flip", Vec::new());
    runtime.function(&mut store, "flip", Vec::new());
    runtime.function(&mut store, "flip", Vec::new());

    runtime.function(&mut store, "get", Vec::new());

    assert_eq!(store.scratch, GetReturn(true).encode());
}

#[test]
fn contract_storage_initializers() {
    #[derive(Debug, PartialEq, Encode, Decode)]
    struct FooReturn {
        value: u32
    }

    // parse
    let (runtime, mut store) = build_solidity("
        contract test {
            uint32 a = 100;
            uint32 b = 200;

            constructor() public {
                b = 300;
            }

            function foo() public returns (uint32) {
                return a + b;
            }
        }",
    );

    runtime.constructor(&mut store, 0, Vec::new());

    runtime.function(&mut store, "foo", Vec::new());

    let ret = FooReturn{ value: 400 };

    assert_eq!(store.scratch, ret.encode());
}

#[test]
fn contract_constants() {
    #[derive(Debug, PartialEq, Encode, Decode)]
    struct FooReturn {
        value: u32
    }

    // parse
    let (runtime, mut store) = build_solidity("
        contract test {
            uint32 constant a = 300 + 100;

            function foo() public pure returns (uint32) {
                uint32 ret = a;
                return ret;
            }
        }",
    );

    runtime.constructor(&mut store, 0, Vec::new());

    runtime.function(&mut store, "foo", Vec::new());

    let ret = FooReturn{ value: 400 };

    assert_eq!(store.scratch, ret.encode());
}
