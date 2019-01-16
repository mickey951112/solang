#![feature(box_patterns)]

extern crate lalrpop;
extern crate num_bigint;
extern crate lalrpop_util;
extern crate llvm_sys;
extern crate num_traits;
extern crate parity_wasm;
extern crate wasmi;
extern crate clap;
extern crate lazy_static;

use clap::{App, Arg};
mod ast;
mod solidity;
mod resolve;
mod emit;
mod link;
mod vartable;
mod test;

use std::fs::File;
use std::io::prelude::*;
use lalrpop_util::ParseError;
use emit::Emitter;

fn main() {
    let matches = App::new("solang")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("Solidity to WASM Compiler")
        .arg(Arg::with_name("INPUT")
            .help("Solidity input files")
            .required(true)
            .multiple(true))
        .arg(Arg::with_name("LLVM")
            .help("emit llvm IR rather than wasm")
            .long("emit-llvm"))
        .get_matches();

    for filename in matches.values_of("INPUT").unwrap() {
        let mut f = File::open(&filename).expect("file not found");

        let mut contents = String::new();
        f.read_to_string(&mut contents)
            .expect("something went wrong reading the file");


        // parse phase
        let nocomments = strip_comments(&contents);

        let s = solidity::SourceUnitParser::new()
            .parse(&nocomments);

        let mut past;

        match s {
            Ok(s) => past = s,
            Err(e) => {
                match e {
                    ParseError::InvalidToken{location} => println!("{}: error: invalid token token at {}", filename, offset_to_line_column(&contents, location)),
                    ParseError::UnrecognizedToken{token, expected} => {
                        match token {
                            None => println!("{}: error: unrecognised token, expected {}", filename, expected.join(",")),
                            Some(t) => println!("{}: error: unrecognised token `{}' from {} to {}", filename, t.1, offset_to_line_column(&contents, t.0), offset_to_line_column(&contents, t.2)),
                        }
                    },
                    ParseError::User{error} => {
                        println!("{}: error: {}", filename, error)
                    },
                    ParseError::ExtraToken{token} => {
                        println!("{}: extra token `{}' encountered at {}-{}", filename, token.1, token.0, token.2)
                    }
                }
                return;
            }
        }

        past.name = filename.to_string();

        // resolve phase
        if let Err(s) = resolve::resolve(&mut past) {
            println!("{}: {}", filename, s);
            break;
        }

        // emit phase
        let res = Emitter::new(past);

        for contract in &res.contracts {
            if matches.is_present("LLVM") {
                contract.dump_llvm();
            } else {
                if let Err(s) = contract.wasm_file(&res, contract.name.to_string() + ".wasm") {
                    println!("error: {}", s);
                }
            }
        }
    }
}

fn offset_to_line_column(s: &String, offset: usize) -> String {
    let mut line = 1;
    let mut column = 1;

    for (o, c) in s.char_indices() {
        if o == offset {
            break;
        }
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    format!("{}:{}", line, column)
}

//
// The lalrpop lexer cannot deal with comments, so you have to write your own lexer.
// Rather than do that let's just strip the comments before passing it to the lexer
// It's not great code but it's a stop-gap solution anyway
fn strip_comments(s: &String) -> String {
    let mut n = String::new();
    let mut single_line = false;
    let mut multi_line = false;
    let mut last = '\0';
    let mut c = '\0';

    for (i, j) in s.char_indices() {
        c = j;
        if single_line {
            if c == '\n' {
                single_line = false;
            }
            last = ' ';
        } else if multi_line {
            if last == '*' && c == '/' {
                c = ' ';
                multi_line = false;
            }
            if last != '\n' {
                last = ' ';
            }
        } else if last == '/' && c == '/'  {
            single_line = true;
            last = ' ';
        } else if last == '/' && c == '*'  {
            multi_line = true;
            last = ' ';
        }

        if i > 0 {
            n.push(last);
        }
        last = c;
    }

    if !single_line && !multi_line {
        n.push(c);
    }

    n
}

#[test]
fn strip_comments_test() {
    assert_eq!(strip_comments(&("foo //Zabc\nbar".to_string())),
                              "foo       \nbar");
    assert_eq!(strip_comments(&("foo /*|x\ny&*/ bar".to_string())),
                              "foo     \n     bar");
}

