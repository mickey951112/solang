use crate::build_solidity;

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
