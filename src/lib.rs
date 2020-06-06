extern crate blake2_rfc;
extern crate clap;
extern crate hex;
extern crate inkwell;
extern crate lalrpop_util;
extern crate lazy_static;
extern crate num_bigint;
extern crate num_derive;
extern crate num_traits;
extern crate parity_wasm;
extern crate phf;
extern crate serde;
extern crate serde_derive;
extern crate tiny_keccak;
extern crate unicode_xid;

pub mod abi;
pub mod codegen;
mod emit;
pub mod link;
pub mod output;
mod parser;
mod sema;

use inkwell::OptimizationLevel;
use std::fmt;

/// The target chain you want to compile Solidity for.
#[derive(PartialEq, Clone, Copy)]
pub enum Target {
    /// Parity Substrate, see https://substrate.dev/
    Substrate,
    /// Ethereum ewasm, see https://github.com/ewasm/design
    Ewasm,
    /// Sawtooth Sabre, see https://github.com/hyperledger/sawtooth-sabre
    Sabre,
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Target::Substrate => write!(f, "Substrate"),
            Target::Ewasm => write!(f, "ewasm"),
            Target::Sabre => write!(f, "Sawtooth Sabre"),
        }
    }
}

/// Compile a solidity file to list of wasm files and their ABIs. The filename is only used for error messages;
/// the contents of the file is provided in the `src` argument.
///
/// This function only produces a single contract and abi, which is compiled for the `target` specified. Any
/// compiler warnings, errors and informational messages are also provided.
///
/// The ctx is the inkwell llvm context.
pub fn compile(
    src: &str,
    filename: &str,
    opt: OptimizationLevel,
    target: Target,
) -> (Vec<(Vec<u8>, String)>, Vec<output::Output>) {
    let ctx = inkwell::context::Context::create();

    let pt = match parser::parse(src) {
        Ok(s) => s,
        Err(errors) => {
            return (Vec::new(), errors);
        }
    };

    // resolve
    let mut ns = sema::sema(pt, target);

    if output::any_errors(&ns.diagnostics) {
        return (Vec::new(), ns.diagnostics);
    }

    // codegen all the contracts
    for contract_no in 0..ns.contracts.len() {
        codegen::codegen(contract_no, &mut ns);
    }

    let results = (0..ns.contracts.len())
        .map(|c| {
            let (abistr, _) = abi::generate_abi(c, &ns, false);

            // codegen
            let contract = emit::Contract::build(&ctx, &ns.contracts[c], &ns, filename, opt);

            let bc = contract.wasm(true).expect("llvm wasm emit should work");

            (bc, abistr)
        })
        .collect();

    (results, ns.diagnostics)
}

/// Parse and resolve the Solidity source code provided in src, for the target chain as specified in target.
/// The result is a list of resolved contracts (if successful) and a list of compiler warnings, errors and
/// informational messages like `found contact N`.
///
/// Note that multiple contracts can be specified in on solidity source file.
pub fn parse_and_resolve(src: &str, target: Target) -> sema::ast::Namespace {
    let pt = match parser::parse(src) {
        Ok(s) => s,
        Err(errors) => {
            let mut ns = sema::ast::Namespace::new(target, 32);

            ns.diagnostics = errors;

            return ns;
        }
    };

    // resolve
    sema::sema(pt, target)
}
