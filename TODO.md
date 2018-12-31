# TODO

Please note that solang so far is the result of a few days of hacking. My aim
was to write something which can compile an extremely simple solidity contract
to wasm.

There are other rust projects that implement a compiler frontend in rust
using llvm, for example [bfc](https://github.com/Wilfred/bfc).

## Commandline:
 * Add proper command line argument parser, so we have --help, --version, -O,
   -Wall, --emit-llvm and -S

## Parser:
 * the AST uses positional structs, these should really be named fields
 * Remove new() methods and make all fields public
 * the lalrpop lexer cannot deal with comments, we need a customer lexer for this rather
   than removing comments in the strip_comments function
 * We should use location tracker so that warnings and errors can carry proper line and column
   numbers
 * Does not parse all of solidity yet

## Resolver:
 * The resolver is very bare-bones right now.
 * Variables need to be stored in scopes and carry their types and initializers
 * Need to walk the CFG to check for uninitialised usses
 * Expressions need to be checked for types, add warnings and errors or casts as appropriate
 * Custom types like mappings and structs need implementing

## Code Emitter/LLVM IR conversion
 * The code emmiter has barely started
 * llvm-sys is unsafe and a clean safe interface would be preferable

## Testing
 * We really need something which can load and test wasm files

## Steps
 - Verify expressions (without assignments or functions calls)
 - Make assignment to variable work
 - Make re-assignment work with pi-nodes
 - Make full control-flow work
 - Make scoped variables work