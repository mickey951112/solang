use parity_scale_codec::Encode;
use parity_scale_codec_derive::{Decode, Encode};
use rand::Rng;

use super::{build_solidity, first_error, no_errors};
use solang::{parse_and_resolve, Target};

#[test]
fn basic_tests() {
    let (_, errors) = parse_and_resolve(
        r#"
        contract foo {
            function foo() public {
                    string f = new string(2);

                    f[0] = 102;
            }
        }"#,
        &Target::Substrate,
    );

    assert_eq!(
        first_error(errors),
        "array subscript is not permitted on string"
    );

    let (_, errors) = parse_and_resolve(
        r#"
        contract foo {
            function foo() public {
                    bytes f = new string(2);
            }
        }"#,
        &Target::Substrate,
    );

    assert_eq!(
        first_error(errors),
        "conversion from string to bytes not possible"
    );

    let (_, errors) = parse_and_resolve(
        r#"
        contract foo {
            function foo() public {
                    string f = new bytes(2);
            }
        }"#,
        &Target::Substrate,
    );

    assert_eq!(
        first_error(errors),
        "conversion from bytes to string not possible"
    );

    let (_, errors) = parse_and_resolve(
        r#"
        contract foo {
            function foo() public {
                    string f = string(new bytes(2));
            }
        }"#,
        &Target::Substrate,
    );

    no_errors(errors);

    let (_, errors) = parse_and_resolve(
        r#"
        contract foo {
            function foo() public {
                    bytes f = bytes(new string(2));
            }
        }"#,
        &Target::Substrate,
    );

    no_errors(errors);
}

#[test]
fn more_tests() {
    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public {
                string s = new string(10);

                assert(s.length == 10);
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public {
                bytes s = new bytes(2);

                s[0] = 0x41;
                s[1] = 0x42;

                assert(s.length == 2);

                assert(s[0] == 0x41);
                assert(s[1] == 0x42);
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function ref_test(bytes n) private {
                n[1] = 102;

                n = new bytes(10);
                // new reference
                n[1] = 104;
            }

            function test() public {
                bytes s = new bytes(2);

                s[0] = 0x41;
                s[1] = 0x42;

                assert(s.length == 2);

                ref_test(s);

                assert(s[0] == 0x41);
                assert(s[1] == 102);
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public {
                bytes s = "ABCD";

                assert(s.length == 4);

                s[0] = 0x41;
                s[1] = 0x42;
                s[2] = 0x43;
                s[3] = 0x44;
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());
}

#[test]
fn string_compare() {
    // compare literal to literal. This should be compile-time thing
    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public {
                assert(hex"414243" == "ABC");

                assert(hex"414243" != "ABD");
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function lets_compare1(string s) private returns (bool) {
                return s == "the quick brown fox jumps over the lazy dog";
            }

            function lets_compare2(string s) private returns (bool) {
                return "the quick brown fox jumps over the lazy dog" == s;
            }

            function test() public {
                string s1 = "the quick brown fox jumps over the lazy dog";

                assert(lets_compare1(s1));
                assert(lets_compare2(s1));

                string s2 = "the quick brown dog jumps over the lazy fox";

                assert(!lets_compare1(s2));
                assert(!lets_compare2(s2));

                assert(s1 != s2);

                s1 = "the quick brown dog jumps over the lazy fox";

                assert(s1 == s2);
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());
}

#[test]
fn string_concat() {
    // concat literal and literal. This should be compile-time thing
    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public {
                assert(hex"41424344" == "AB" + "CD");
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public {
                string s1 = "x";
                string s2 = "asdfasdf";

                assert(s1 + " foo" == "x foo");
                assert("bar " + s1 == "bar x");

                assert(s1 + s2 == "xasdfasdf");
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());
}

#[test]
fn string_abi_encode() {
    #[derive(Debug, PartialEq, Encode, Decode)]
    struct Val(String);

    #[derive(Debug, PartialEq, Encode, Decode)]
    struct Ret3([i8; 4], String, bool);

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public returns (string) {
                return "foobar";
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());

    assert_eq!(store.scratch, Val("foobar".to_string()).encode());

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test() public returns (int8[4], string, bool) {
                return ([ int8(120), 3, -127, 64], "Call me Ishmael. Some years ago—never mind how long precisely—having little or no money in my purse, and nothing particular to interest me on shore, I thought I would sail about a little and see the watery part of the world. It is a way I have of driving off the spleen and regulating the circulation. Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul; whenever I find myself involuntarily pausing before coffin warehouses, and bringing up the rear of every funeral I meet; and especially whenever my hypos get such an upper hand of me, that it requires a strong moral principle to prevent me from deliberately stepping into the street, and methodically knocking people’s hats off—then, I account it high time to get to sea as soon as I can. This is my substitute for pistol and ball. With a philosophical flourish Cato throws himself upon his sword; I quietly take to the ship. There is nothing surprising in this. If they but knew it, almost all men in their degree, some time or other, cherish very nearly the same feelings towards the ocean with me.",
                true);
            }
        }"##,
    );

    runtime.function(&mut store, "test", Vec::new());

    assert_eq!(store.scratch, Ret3([ 120, 3, -127, 64], "Call me Ishmael. Some years ago—never mind how long precisely—having little or no money in my purse, and nothing particular to interest me on shore, I thought I would sail about a little and see the watery part of the world. It is a way I have of driving off the spleen and regulating the circulation. Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul; whenever I find myself involuntarily pausing before coffin warehouses, and bringing up the rear of every funeral I meet; and especially whenever my hypos get such an upper hand of me, that it requires a strong moral principle to prevent me from deliberately stepping into the street, and methodically knocking people’s hats off—then, I account it high time to get to sea as soon as I can. This is my substitute for pistol and ball. With a philosophical flourish Cato throws himself upon his sword; I quietly take to the ship. There is nothing surprising in this. If they but knew it, almost all men in their degree, some time or other, cherish very nearly the same feelings towards the ocean with me.".to_string(), true).encode());
}

#[test]
fn string_abi_decode() {
    #[derive(Debug, PartialEq, Encode, Decode)]
    struct Val(String);

    #[derive(Debug, PartialEq, Encode, Decode)]
    struct ValB(Vec<u8>);

    // we should try lengths: 0 to 63, 64 to 0x800
    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            function test(string s) public returns (string){
                return " " + s + " ";
            }
        }"##,
    );

    let moby_dick_first_para = "Call me Ishmael. Some years ago—never mind how long precisely—having little or no money in my purse, and nothing particular to interest me on shore, I thought I would sail about a little and see the watery part of the world. It is a way I have of driving off the spleen and regulating the circulation. Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul; whenever I find myself involuntarily pausing before coffin warehouses, and bringing up the rear of every funeral I meet; and especially whenever my hypos get such an upper hand of me, that it requires a strong moral principle to prevent me from deliberately stepping into the street, and methodically knocking people’s hats off—then, I account it high time to get to sea as soon as I can. This is my substitute for pistol and ball. With a philosophical flourish Cato throws himself upon his sword; I quietly take to the ship. There is nothing surprising in this. If they but knew it, almost all men in their degree, some time or other, cherish very nearly the same feelings towards the ocean with me.";

    runtime.function(&mut store, "test", Val("foobar".to_string()).encode());
    assert_eq!(store.scratch, Val(" foobar ".to_string()).encode());

    runtime.function(
        &mut store,
        "test",
        Val(moby_dick_first_para.to_string()).encode(),
    );

    assert_eq!(
        store.scratch,
        Val(format!(" {} ", moby_dick_first_para)).encode()
    );

    let mut rng = rand::thread_rng();

    for len in 0x4000 - 10..0x4000 + 10 {
        let mut s = Vec::new();

        s.resize(len, 0);

        rng.fill(&mut s[..]);

        let (runtime, mut store) = build_solidity(
            r##"
            contract foo {
                function test(bytes s) public returns (bytes){
                    return hex"fe" + s;
                }
            }"##,
        );

        let arg = ValB(s.clone()).encode();

        runtime.function(&mut store, "test", arg.clone());

        s.insert(0, 0xfeu8);

        let ret = ValB(s).encode();

        assert_eq!(store.scratch, ret);
    }
}

#[test]
fn string_storage() {
    #[derive(Debug, PartialEq, Encode, Decode)]
    struct Val(String);

    let (runtime, mut store) = build_solidity(
        r##"
        contract foo {
            string bar;

            function set_bar() public {
                bar = "foobar";
            }

            function get_bar() public returns (string) {
                return bar;
            }

        }"##,
    );

    runtime.function(&mut store, "set_bar", Vec::new());

    assert_eq!(store.store.get(&[0u8; 32]).unwrap(), b"foobar");

    runtime.function(&mut store, "get_bar", Vec::new());

    assert_eq!(store.scratch, Val("foobar".to_string()).encode());
}
