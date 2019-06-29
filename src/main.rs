extern crate clap;
extern crate ethabi;
extern crate ethereum_types;
extern crate hex;
extern crate lalrpop_util;
extern crate lazy_static;
extern crate llvm_sys;
extern crate num_bigint;
extern crate num_traits;
extern crate parity_wasm;
extern crate serde;
extern crate tiny_keccak;
extern crate unescape;
extern crate wasmi;

use clap::{App, Arg};
mod ast;
mod cfg;
mod emit;
mod link;
mod output;
mod parser;
mod resolver;
mod solidity;
mod test;

use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;

#[derive(Serialize)]
pub struct EwasmContract {
    pub wasm: String,
}

#[derive(Serialize)]
pub struct JsonContract {
    abi: Vec<resolver::ABI>,
    ewasm: EwasmContract,
}

#[derive(Serialize)]
pub struct JsonResult {
    pub errors: Vec<output::OutputJson>,
    pub contracts: HashMap<String, HashMap<String, JsonContract>>,
}

fn main() {
    let matches = App::new("solang")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::with_name("INPUT")
                .help("Solidity input files")
                .required(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("CFG")
                .help("emit Control Flow Graph")
                .long("emit-cfg")
                .group("EMIT"),
        )
        .arg(
            Arg::with_name("LLVM")
                .help("emit llvm IR rather than WASM")
                .long("emit-llvm")
                .group("EMIT"),
        )
        .arg(
            Arg::with_name("LLVM-BC")
                .help("emit llvm BC rather than WASM")
                .long("emit-bc")
                .group("EMIT"),
        )
        .arg(
            Arg::with_name("OBJECT")
                .help("emit WASM object file")
                .long("emit-object")
                .group("EMIT"),
        )
        .arg(
            Arg::with_name("STD-JSON")
                .help("mimic solidity json output on stdout")
                .long("standard-json")
                .group("EMIT"),
        )
        .arg(
            Arg::with_name("VERBOSE")
                .help("show verbose messages")
                .short("v")
                .long("verbose"),
        )
        .get_matches();

    let mut fatal = false;
    let mut json = JsonResult {
        errors: Vec::new(),
        contracts: HashMap::new(),
    };

    for filename in matches.values_of("INPUT").unwrap() {
        let mut f = File::open(&filename).expect("file not found");

        let mut contents = String::new();
        f.read_to_string(&mut contents)
            .expect("something went wrong reading the file");

        let past = match parser::parse(&contents) {
            Ok(s) => s,
            Err(errors) => {
                if matches.is_present("STD-JSON") {
                    let mut out = output::message_as_json(filename, &contents, &errors);
                    json.errors.append(&mut out);
                } else {
                    output::print_messages(
                        filename,
                        &contents,
                        &errors,
                        matches.is_present("VERBOSE"),
                    );
                    fatal = true;
                }
                continue;
            }
        };

        // resolve phase
        let (contracts, errors) = resolver::resolver(past);

        if matches.is_present("STD-JSON") {
            let mut out = output::message_as_json(filename, &contents, &errors);
            json.errors.append(&mut out);
        } else {
            output::print_messages(filename, &contents, &errors, matches.is_present("VERBOSE"));
        }

        if contracts.is_empty() {
            continue;
        }

        let mut json_contracts = HashMap::new();

        // emit phase
        for contract in &contracts {
            if matches.is_present("CFG") {
                println!("{}", contract.to_string());
                continue;
            }

            let abi = contract.generate_abi();

            let contract = emit::Contract::new(contract, &filename);

            if matches.is_present("LLVM") {
                contract.dump_llvm();
                continue;
            }

            if matches.is_present("LLVM-BC") {
                let bc = contract.bitcode();
                let bc_filename = contract.name.to_string() + ".bc";

                let mut file = File::create(bc_filename).unwrap();
                file.write_all(&bc).unwrap();
                continue;
            }

            let obj = match contract.wasm() {
                Ok(o) => o,
                Err(s) => {
                    println!("error: {}", s);
                    std::process::exit(1);
                }
            };

            if matches.is_present("OBJECT") {
                let obj_filename = contract.name.to_string() + ".o";

                let mut file = File::create(obj_filename).unwrap();
                file.write_all(&obj).unwrap();
                continue;
            }

            let wasm = link::link(&obj);

            if matches.is_present("STD-JSON") {
                json_contracts.insert(
                    contract.name.to_owned(),
                    JsonContract {
                        abi,
                        ewasm: EwasmContract {
                            wasm: hex::encode_upper(wasm),
                        },
                    },
                );
            } else {
                let wasm_filename = contract.name.to_string() + ".wasm";

                let mut file = File::create(wasm_filename).unwrap();
                file.write_all(&wasm).unwrap();

                let abi_filename = contract.name.to_string() + ".abi";

                file = File::create(abi_filename).unwrap();
                file.write_all(serde_json::to_string(&abi).unwrap().as_bytes())
                    .unwrap();
            }
        }

        json.contracts.insert(filename.to_owned(), json_contracts);
    }

    if matches.is_present("STD-JSON") {
        println!("{}", serde_json::to_string(&json).unwrap());
    } else if fatal {
        std::process::exit(1);
    }
}
