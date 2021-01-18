use crate::build_solidity;

#[test]
fn string() {
    let mut vm = build_solidity(
        r#"
        contract foo {
            string s;

            function set(string value) public {
                s = value;
            }

            function get() public returns (string) {
                return s;
            }
        }"#,
    );

    vm.constructor(&[]);

    assert_eq!(
        vm.data[0..12].to_vec(),
        vec![65, 177, 160, 100, 12, 0, 0, 0, 0, 0, 0, 0]
    );

    let returns = vm.function("get", &[]);

    assert_eq!(returns, vec![ethabi::Token::String(String::from(""))]);

    vm.function(
        "set",
        &[ethabi::Token::String(String::from("Hello, World!"))],
    );

    assert_eq!(
        vm.data[0..12].to_vec(),
        vec![65, 177, 160, 100, 12, 0, 0, 0, 28, 0, 0, 0]
    );

    assert_eq!(vm.data[28..41].to_vec(), b"Hello, World!");

    let returns = vm.function("get", &[]);

    assert_eq!(
        returns,
        vec![ethabi::Token::String(String::from("Hello, World!"))]
    );

    // try replacing it with a string of the same length. This is a special
    // fast-path handling
    vm.function(
        "set",
        &[ethabi::Token::String(String::from("Hallo, Werld!"))],
    );

    let returns = vm.function("get", &[]);

    assert_eq!(
        returns,
        vec![ethabi::Token::String(String::from("Hallo, Werld!"))]
    );

    assert_eq!(
        vm.data[0..12].to_vec(),
        vec![65, 177, 160, 100, 12, 0, 0, 0, 28, 0, 0, 0]
    );

    // Try setting this to an empty string. This is also a special case where
    // the result should be offset 0
    vm.function("set", &[ethabi::Token::String(String::from(""))]);

    let returns = vm.function("get", &[]);

    assert_eq!(returns, vec![ethabi::Token::String(String::from(""))]);

    assert_eq!(
        vm.data[0..12].to_vec(),
        vec![65, 177, 160, 100, 12, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn bytes() {
    let mut vm = build_solidity(
        r#"
        contract c {
            bytes foo;

            function set_foo(bytes bs) public {
                foo = bs;
            }

            function foo_length() public returns (uint32) {
                return foo.length;
            }

            function set_foo_offset(uint32 index, byte b) public {
                foo[index] = b;
            }

            function get_foo_offset(uint32 index) public returns (byte) {
                return foo[index];
            }
        }"#,
    );

    vm.constructor(&[]);

    assert_eq!(
        vm.data[0..12].to_vec(),
        vec![11, 66, 182, 57, 12, 0, 0, 0, 0, 0, 0, 0]
    );

    let returns = vm.function("foo_length", &[]);

    assert_eq!(
        returns,
        vec![ethabi::Token::Uint(ethereum_types::U256::from(0))]
    );

    vm.function(
        "set_foo",
        &[ethabi::Token::Bytes(
            b"The shoemaker always wears the worst shoes".to_vec(),
        )],
    );

    assert_eq!(
        vm.data[0..12].to_vec(),
        vec![11, 66, 182, 57, 12, 0, 0, 0, 28, 0, 0, 0]
    );

    for (i, b) in b"The shoemaker always wears the worst shoes"
        .to_vec()
        .into_iter()
        .enumerate()
    {
        let returns = vm.function(
            "get_foo_offset",
            &[ethabi::Token::Uint(ethereum_types::U256::from(i))],
        );

        assert_eq!(returns, vec![ethabi::Token::FixedBytes(vec![b])]);
    }

    vm.function(
        "set_foo_offset",
        &[
            ethabi::Token::Uint(ethereum_types::U256::from(2)),
            ethabi::Token::FixedBytes(b"E".to_vec()),
        ],
    );

    vm.function(
        "set_foo_offset",
        &[
            ethabi::Token::Uint(ethereum_types::U256::from(7)),
            ethabi::Token::FixedBytes(b"E".to_vec()),
        ],
    );

    for (i, b) in b"ThE shoEmaker always wears the worst shoes"
        .to_vec()
        .into_iter()
        .enumerate()
    {
        let returns = vm.function(
            "get_foo_offset",
            &[ethabi::Token::Uint(ethereum_types::U256::from(i))],
        );

        assert_eq!(returns, vec![ethabi::Token::FixedBytes(vec![b])]);
    }
}

#[test]
#[should_panic]
fn bytes_set_subscript_range() {
    let mut vm = build_solidity(
        r#"
        contract c {
            bytes foo;

            function set_foo(bytes bs) public {
                foo = bs;
            }

            function foo_length() public returns (uint32) {
                return foo.length;
            }

            function set_foo_offset(uint32 index, byte b) public {
                foo[index] = b;
            }

            function get_foo_offset(uint32 index) public returns (byte) {
                return foo[index];
            }
        }"#,
    );

    vm.constructor(&[]);

    vm.function(
        "set_foo_offset",
        &[
            ethabi::Token::Uint(ethereum_types::U256::from(0)),
            ethabi::Token::FixedBytes(b"E".to_vec()),
        ],
    );
}

#[test]
#[should_panic]
fn bytes_get_subscript_range() {
    let mut vm = build_solidity(
        r#"
        contract c {
            bytes foo;

            function set_foo(bytes bs) public {
                foo = bs;
            }

            function foo_length() public returns (uint32) {
                return foo.length;
            }

            function set_foo_offset(uint32 index, byte b) public {
                foo[index] = b;
            }

            function get_foo_offset(uint32 index) public returns (byte) {
                return foo[index];
            }
        }"#,
    );

    vm.constructor(&[]);

    vm.function(
        "set_foo",
        &[ethabi::Token::Bytes(
            b"The shoemaker always wears the worst shoes".to_vec(),
        )],
    );

    vm.function(
        "get_foo_offset",
        &[ethabi::Token::Uint(ethereum_types::U256::from(
            0x80000000u64,
        ))],
    );
}
