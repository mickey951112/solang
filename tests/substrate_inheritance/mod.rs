extern crate solang;

use super::{build_solidity, first_error, no_errors, parse_and_resolve};
use solang::file_cache::FileCache;
use solang::Target;

#[test]
fn test_virtual() {
    let ns = parse_and_resolve(
        r#"
        contract c {        
            function test() public;
        }"#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "function with no body must be marked ‘virtual’"
    );

    let ns = parse_and_resolve(
        r#"
        contract c {        
            function test() virtual public {}
        }"#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "function marked ‘virtual’ cannot have a body"
    );

    let ns = parse_and_resolve(
        r#"
        contract c {
            function test() virtual public;
            function test2() virtual public;
        }"#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "contract should be marked ‘abstract contract’ since it has 2 virtual functions"
    );
}

#[test]
fn test_abstract() {
    let ns = parse_and_resolve(
        r#"
        abstract contract foo {
            constructor(int arg1) public {
            }
        
            function f1() public {
            }
        }
        
        contract bar {
            function test() public {
                foo x = new foo(1);
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "cannot construct ‘foo’ of type ‘abstract contract’"
    );

    let ns = parse_and_resolve(
        r#"
        abstract contract foo {
            constructor(int arg1) public {
            }
        
            function f1() public {
            }
        }
        
        contract bar {
            function test() public {
                foo x = new foo({arg: 1});
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "cannot construct ‘foo’ of type ‘abstract contract’"
    );

    let mut cache = FileCache::new();

    cache.set_file_contents(
        "a.sol".to_string(),
        r#"
        abstract contract foo {
            constructor(int arg1) public {
            }
        
            function f1() public {
            }
        }
        
        contract bar {
            function test() public returns (uint32) {
                return 102;
            }
        }
        "#
        .to_string(),
    );

    let (contracts, ns) = solang::compile(
        "a.sol",
        &mut cache,
        inkwell::OptimizationLevel::Default,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    assert_eq!(contracts.len(), 1);
}

#[test]
fn test_interface() {
    let ns = parse_and_resolve(
        r#"
        interface foo {
            constructor(int arg1) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "constructor not allowed in an interface"
    );

    let ns = parse_and_resolve(
        r#"
        interface foo {
            function bar() external {}
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "functions can not have bodies in an interface"
    );

    let ns = parse_and_resolve(
        r#"
        interface foo {
            function bar() virtual private;
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "functions must be declared ‘external’ in an interface"
    );

    let ns = parse_and_resolve(
        r#"
        interface bar {
            function foo() virtual internal;
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "functions must be declared ‘external’ in an interface"
    );
}

#[test]
fn inherit() {
    let ns = parse_and_resolve(
        r#"
        contract a is a {
            constructor(int arg1) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "contract ‘a’ cannot inherit itself"
    );

    let ns = parse_and_resolve(
        r#"
        contract a is foo {
            constructor(int arg1) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(first_error(ns.diagnostics), "contract ‘foo’ not found");

    let ns = parse_and_resolve(
        r#"
        contract a is b {
            constructor(int arg1) public {
            }
        }

        contract b is a {
            constructor(int arg1) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "inheriting ‘a’ from contract ‘b’ is cyclic"
    );

    let ns = parse_and_resolve(
        r#"
        contract a {
            constructor(int arg1) public {
            }
        }

        contract b is a, a {
            constructor(int arg1) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "contract ‘b’ duplicate inherits ‘a’"
    );

    let ns = parse_and_resolve(
        r#"
        contract a is b {
            constructor(int arg1) public {
            }
        }

        contract b is c {
            constructor(int arg1) public {
            }
        }

        contract c is a {
            constructor(int arg1) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "inheriting ‘a’ from contract ‘c’ is cyclic"
    );

    let ns = parse_and_resolve(
        r#"
        contract a is b {
            constructor(int arg1) public {
            }
        }

        contract b is c {
            constructor(int arg1) public {
            }
        }

        contract d {
            constructor(int arg1) public {
            }
        }

        contract c is d, a {
            constructor(int arg1) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(
        first_error(ns.diagnostics),
        "inheriting ‘a’ from contract ‘c’ is cyclic"
    );
}

#[test]
fn inherit_types() {
    let ns = parse_and_resolve(
        r#"
        contract a is b {
            function test() public returns (enum_x) {
                return enum_x.x2;
            }
        }

        contract b {
            enum enum_x { x1, x2 }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let ns = parse_and_resolve(
        r#"
        contract a is b {
            function test() public returns (enum_x) {
                return enum_x.x2;
            }

            function test2() public returns (enum_y) {
                return enum_y.y2;
            }
        }

        contract b is c {
            enum enum_y { y1, y2 }
        }

        contract c {
            enum enum_x { x1, x2 }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let ns = parse_and_resolve(
        r#"
        contract a is b, c {
            function test() public returns (enum_x) {
                return enum_x.x2;
            }

            function test2() public returns (enum_y) {
                return enum_y.y2;
            }
        }

        contract b is c {
            enum enum_y { y1, y2 }
        }

        contract c {
            enum enum_x { x1, x2 }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let ns = parse_and_resolve(
        r#"
        contract a {
            function test() public returns (enum_x) {
                return enum_x.x2;
            }
        }

        contract b {
            enum enum_x { x1, x2 }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(first_error(ns.diagnostics), "type ‘enum_x’ not found");

    let ns = parse_and_resolve(
        r#"
        contract a is b {
            foo public var1;
        }

        contract b {
            struct foo {
                uint32 f1;
                uint32 f2;
            }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let ns = parse_and_resolve(
        r#"
        contract b {
            struct foo {
                uint32 f1;
                uint32 f2;
            }
        }

        contract c {
            enum foo { f1, f2 }
        }

        contract a is b, c {
            function test(foo x) public {
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(first_error(ns.diagnostics), "already defined ‘foo’");
}

#[test]
fn inherit_variables() {
    let ns = parse_and_resolve(
        r#"
        contract b {
            int public foo;
        }
        
        contract c is b {
            function getFoo() public returns (int) {
                return foo;
            }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let ns = parse_and_resolve(
        r#"
        contract b {
            int private foo;
        }
        
        contract c is b {
            function getFoo() public returns (int) {
                return foo;
            }
        }
        "#,
        Target::Substrate,
    );

    assert_eq!(first_error(ns.diagnostics), "`foo\' is not declared");

    let ns = parse_and_resolve(
        r#"
        contract a {
            int public foo;
        }

        contract b is a {
            int public bar;
        }

        contract c is b {
            function getFoo() public returns (int) {
                return foo;
            }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let ns = parse_and_resolve(
        r#"
        contract a {
            int private foo;
        }

        contract b is a {
            int public foo;
        }

        contract c is b {
            function getFoo() public returns (int) {
                return foo;
            }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let ns = parse_and_resolve(
        r#"
        contract a {
            int public constant foo = 0xbffe;
        }

        contract c is a {
            function getFoo() public returns (int) {
                return foo;
            }
        }
        "#,
        Target::Substrate,
    );

    no_errors(ns.diagnostics);

    let mut runtime = build_solidity(
        r##"
        contract b is a {
            uint16 public foo = 65535;
        }

        contract a {
            uint16 private foo = 102;
        }"##,
    );

    runtime.constructor(0, Vec::new());

    let mut slot = [0u8; 32];

    assert_eq!(
        runtime.store.get(&(runtime.vm.address, slot)).unwrap(),
        &vec!(102, 0)
    );

    slot[0] = 1;

    assert_eq!(
        runtime.store.get(&(runtime.vm.address, slot)).unwrap(),
        &vec!(0xff, 0xff)
    );

    let mut runtime = build_solidity(
        r##"
        contract b is a {
            uint16 public var_b;

            function test() public {
                var_a = 102;
                var_b = 65535;
            }
        }

        contract a {
            uint16 public var_a;
        }"##,
    );

    runtime.constructor(0, Vec::new());
    runtime.function("test", Vec::new());

    let mut slot = [0u8; 32];

    assert_eq!(
        runtime.store.get(&(runtime.vm.address, slot)).unwrap(),
        &vec!(102, 0)
    );

    slot[0] = 1;

    assert_eq!(
        runtime.store.get(&(runtime.vm.address, slot)).unwrap(),
        &vec!(0xff, 0xff)
    );
}
