Solidity Language
=================

The Solidity language supported by Solang is compatible with the
`Ethereum Foundation Solidity Compiler <https://github.com/ethereum/solidity/>`_ with
these caveats:

- At this point Solang is very much a work in progress; not at all features
  are supported yet.

- Solang can target different blockchains and some features depending on the target.
  For example, Parity Substrate uses a different ABI encoding and allows constructors
  to be overloaded.

- Solang generates WebAssembly rather than EVM. This means that the ``assembly {}``
  statement using EVM instructions is not supported, and probably never will be.

.. note::

  Where differences exist between different targets or the Ethereum Foundation Solidity
  compiler, this is noted in boxes like these.

Solidity Source File Structure
------------------------------

A Solidity source file may have multiple contracts in them. A contract is defined
with the ``contract`` keyword, following by the contract name and then the definition
of the contract in curly braces ``{ }``. Multiple contracts maybe defined in one solidity
source file. The name of the contract does not have to match the name of the file,
although it this might be convenient.

.. code-block:: javascript

  contract A {
      /// foo simply returns true
      function foo() public return (bool) {
          return true;
      }
  }

  contract B {
      /// bar simply returns false
      function bar() public return (bool) {
          return false;
      }
  }

When compiling this, Solang will output ``A.wasm`` and ``B.wasm``, along with the ABI
files for each contract.

Often, Solidity source files start with a ``pragma solidity`` which specifies the Ethereum
Foundation Solidity compiler version which is permitted to compile this code. Solang does
not follow the Ethereum Foundation Solidity compiler version numbering scheme, so these
pragma statements are silently ignored. There is no need for a ``pragma solidity`` statement
when using Solang.

.. code-block:: javascript

  pragma solidity >=0.4.0 <0.4.8;

All other pragma statements are ignored, but generate warnings. Pragma must be terminated with
a semicolon.

.. note::

  The Ethereum Foundation Solidity compiler can also contain other other elements other than
  contracts: ``import``, ``library``, ``interface``. These are not supported yet.

Types
-----

The following primitive types are supported.

Boolean Type
____________

``bool``
  This represents a single value which can be either ``true`` or ``false``.

Integer Types
_____________

``uint``
  This represents a single unsigned integer of 256 bits wide. Values can be for example
  ``0``, ``102``, ``0xdeadcafe``, or ``1000_000_000_000_000``.

``uint64``, ``uint32``, ``uint16``, ``uint8``
  These represent shorter single unsigned integers of the given width. These widths are
  most efficient in WebAssembly and should be used whenever possible.

``uintN``
  These represent shorter single unsigned integers of width ``N``. ``N`` can be anything
  between 8 and 256 bits.

``int``
  This represents a single signed integer of 256 bits wide. Values can be for example
  ``-102``, ``0``, ``102`` or ``-0xdead_cafe``.

``int64``, ``uint32``, ``uint16``, ``uint8``
  These represent shorter single signed integers of the given width. These widths are
  most efficient in WebAssembly and should be used whenever possible.

``intN``
  These represent shorter single signed integers of width ``N``. ``N`` can be anything
  between 8 and 256 bits.

Underscores ``_`` are allowed in numbers, as long as the number does not start with
an underscore. This means that ``1_000`` is allowed but ``_1000`` is not. Similarly
``0xffff_0000`` is fine, but ``0x_f`` is not.

Assigning values which cannot fit into the type gives a compiler error. For example::

    uint8 foo = 300;

The largest value an ``uint8`` can hold is (2 :superscript:`8`) - 1 = 255. So, the compiler says:

.. code-block:: none

    implicit conversion would truncate from uint16 to uint8


.. tip::

  When using integers, whenever possible use the ``int64``, ``int32`` or ``uint64``,
  ``uint32`` types.

  The Solidity language has its origins for the Ethereum Virtual Machine (EVM), which has
  support for 256 bit registers. Most common CPUs like x86_64 do not implement arithmetic
  for such large types, and any EVM virtual machine implementation has to do bigint
  calculations, which are expensive.

  WebAssembly does not support this. This means that Solang has to emulate larger types with
  many WebAssembly instructions, resulting in larger contract code and higher gas cost.

Fixed Length byte arrays
________________________

Solidity has a primitive type unique to the language. It is a fixed-length byte array of 1 to 32
bytes, declared with *bytes* followed by the array length, for example:
``bytes32``, ``bytes24``, ``bytes8``, or ``bytes1``. ``byte`` is an alias for ``byte1``, so
``byte`` is an array of 1 element. The arrays can be initialized with either a hex string or
a text string.

.. code-block:: javascript

  bytes4 foo = "ABCD";
  bytes4 bar = hex"41_42_43_44";

The ascii value for ``A`` is 41, when written in hexidecimal. So, in this case, foo and bar
are initialized to the same value. Underscores are allowed in hex strings; they exist for
readability. If the string is shorter than the type, it is padded with zeros. For example:

.. code-block:: javascript

  bytes6 foo = "AB" "CD";
  bytes5 bar = hex"41";

String literals can be concatenated like they can in C or C++. Here the types are longer than
the initializers; this means they are padded at the end with zeros. foo will contain the following
bytes in hexidecimal ``41 42 43 44 00 00`` and bar will be ``41 00 00 00 00``.

These types can be used with all the bitwise operators, ``~``, ``|``, ``&``, ``^``, ``<<``, and
``>>``. When these operators are used, the type behaves like an unsigned integer type. In this case
think the type not as an array but as a long number. For example, it is possible to shift by one bit:

.. code-block:: javascript

  bytes2 foo = hex"0101" << 1;
  // foo is 02 02

Since this is an array type, it is possible to read array elements too. They are indexed from zero.
It is not permitted to set array elements; the value of a bytesN type can only be changed
by setting the entire array value.

.. code-block:: javascript

  bytes6 wake_code = "heotymeo";
  bytes1 second_letter = wake_code[1]; // second_letter is "e"

The length can be read using the ``.length`` member variable. Since this is a fixed size array, this
is always the length of the type itself.

.. code-block:: javascript

  bytes32 hash;
  assert(hash.length == 32);
  byte b;
  assert(b.length == 1);

Address Type
____________

The ``address`` type holds the address of an account. It can be initialized with a particular
hexidecimal number, called an address literal. Here is an example:

.. code-block:: javascript

  address foo = 0xE9430d8C01C4E4Bb33E44fd7748942085D82fC91;

The hexidecimal string has to have 40 characters, and not contain any underscores.
The capitalization, i.e. whether ``a`` to ``f`` values are capitalized, is important.
It is defined in
`EIP-55 <https://github.com/ethereum/EIPs/blob/master/EIPS/eip-55.md>`_. For example,
when compiling:

.. code-block:: javascript

  address foo = 0xe9430d8C01C4E4Bb33E44fd7748942085D82fC91;

Since the hexidecimal string is 40 characters without underscores, and the string does
not match the EIP-55 encoding, the compiler will refused to compile this. To make this
a regular hexidecimal number, not an address, add some leading zeros or some underscores.
To make this an address, the compiler error message will give the correct capitalization:

.. code-block:: none

  error: address literal has incorrect checksum, expected ‘0xE9430d8C01C4E4Bb33E44fd7748942085D82fC91’

``address`` cannot be used in any arithmetic or bitwise operations. However, it can be cast to and from
bytes types and integer types and ``==`` and ``!=`` works for comparing two address types.

.. code-block:: javascript

  address foo = address(0);

Enums
_____

Solidity enums types have to be defined on the contract level. An enum has a type name, and a list of
unique values. Enum types can used in public functions, but the value is represented as a ``uint8``
in the ABI.

.. code-block:: javascript

  contract enum_example {
      enum Weekday { Monday, Tuesday, Wednesday, Thursday, Friday, Saturday, Sunday }

      function is_weekend(Weekday day) public pure returns (bool) {
          return (day == Weekday.Saturday || day == Weekday.Sunday);
      }
  }

An enum can be converted to and from integer, but this requires an explicit cast. The value of an enum
is numbered from 0, like in C and Rust.

.. note::

  The Ethereum Foundation Solidity compiler supports additional data types:
  bytes and string. These will be implemented in Solang in early 2020.

Struct Type
___________

A struct is composite type of several other types. This is used to group related items together. before
a struct can be used, the struct must be defined. Then the name of the struct can then be used as a
type itself. For example:

.. code-block:: javascript

  contract deck {
      enum suit { club, diamonds, hearts, spades }
      enum value { two, three, four, five, six, seven, eight, nine, ten, jack, queen, king, ace }
      struct card {
          value v;
          suit s;
      }

      function score(card s) public returns (uint32 score) {
          if (s.suit == suit.hearts) {
              if (s.value == value.ace) {
                  score = 14;
              }
              if (s.value == value.king) {
                  score = 13;
              }
              if (s.value == value.queen) {
                  score = 12;
              }
              if (s.value == value.jack) {
                  score = 11;
              }
          }
          // all others score 0
      }
  }

A struct has one or more fields, each with a unique name. Structs can be function arguments and return
values. Structs can contain other structs. There is a struct literal syntax to create a struct with
all the fields set.

.. code-block:: javascript

  contract deck {
      enum suit { club, diamonds, hearts, spades }
      enum value { two, three, four, five, six, seven, eight, nine, ten, jack, queen, king, ace }
      struct card {
          value v;
          suit s;
      }

      card card1 = card(value.two, suit.club);
      card card2 = card({s: suit.club, v: value.two});

      // This function does a lot of copying
      function set_card1(card c) public returns (card previous) {
          previous = card1;
          card1 = c;
      }
  }

The two contract storage variables ``card1`` and ``card2`` have initializers using struct literals. Struct
literals can either set fields by their position, or field name. In either syntax, all the fields must
be specified. When specifying structs fields by position, it is more likely that the wrong field gets
set to the wrong value. In the example of the card, if the order is wrong then the compiler will give
an errors because the field type does no match; setting a ``suit`` enum field with ``value`` enum
is not permitted. However, if both fields were the of the same type, then the compiler would have no
way of knowing if the fields are in the intended order.

Structs can be contract storage variables. Structs in contract storage can be assigned to structs
in memory and vice versa, like in the *set_card1()* function. Copying structs is expensive; code has
to be generated for each field and executed.

- The function argument ``c`` has to ABI decoded (1 copy + decoding overhead)
- The ``card1`` has to load from contract storage (1 copy + contract storage overhead)
- The ``c`` has to be stored into contract storage (1 copy + contract storage overhead)
- The ``pervious`` struct has to ABI encoded (1 copy + encoding overhead)

Note that struct variables are references. When contract struct variables or normal struct variables
are passed around, just the memory address or storage slot is passed around internally. This makes
it very cheap, but it does mean that if the called function modifies the struct, then this is
visible in the callee as well.

.. code-block:: javascript

  context foo {
      struct bar {
          bytes32 f1;
          bytes32 f2;
          bytes32 f3;
          bytes32 f4;
      }

      function f(struct bar b) public {
          b.f4 = hex"foobar";
      }

      function example() public {
          bar bar1;

          // bar1 is passed by reference; just its address is passed
          f(bar1);

          assert(bar.f4 == hex"foobar");
      }
  }

.. note::
  
  In the Ethereum Foundation Solidity compiler, you need to add ``pragma experimental ABIEncoderV2;``
  to use structs as return values or function arguments in public functions. The default ABI encoder
  of Solang can handle structs, so there is no need for this pragma. The Solang compiler ignores
  this pragma if present.

Fixed Length Arrays
___________________

Arrays can be declared by adding [length] to the type name, where length is a
constant. Any type can be made into an array, including arrays themselves (also
known as arrays of arrays). For example:

.. code-block:: javascript

    contract foo {
        /// In a vote with 11 voters, do the ayes have it?
        function f(bool[11] votes) public pure returns (bool) {
            uint32 i;
            uint32 ayes = 0;

            for (i=0; i<votes.length; i++) {
                if (votes[i]) {
                    ayes += 1;
                }
            }

            // votes.length is odd; integer truncation means that 11 / 2 = 5
            return ayes > votes.length / 2;
        }
    }

Note the length of the array can be read with the ``.length`` member. The length is readonly.
Arrays can be initialized with an array literal. The first element of the array should be
cast to the correct element type. For example:

.. code-block:: javascript

    contract primes {
        uint64[10] constant primes = [ uint64(2), 3, 5, 7, 11, 13, 17, 19, 23, 29 ];
        
        function primenumber(uint32 n) public pure returns (uint64) {
            return primes[n];
        }
    }

Any array subscript which is out of bounds (either an negative array index, or an index past the
last element) will cause a runtime exception. In this example, calling ``primenumber(10)`` will
fail; the first prime number is indexed by 0, and the last by 9.

Arrays are passed by reference. This means that if you modify the array in another function,
those changes will be reflected in the current function. For example:

.. code-block:: javascript

    contract reference {
        function set_2(int8[4] a) pure private {
            a[2] = 102;
        }

        function foo() private {
            int8[4] val = [ int8(1), 2, 3, 4 ];

            set_2(val);

            // val was passed by reference, so was modified
            assert(val[2] == 102);
        }
    }

.. note::

  In Solidity, an fixed array of 32 bytes (or smaller) can be declared as ``bytes32`` or
  ``int8[32]``. In the Ethereum ABI encoding, an ``int8[32]`` is encoded using
  32 × 32 = 1024 bytes. This is because the Ethereum ABI encoding pads each primitive to
  32 bytes. However, since ``bytes32`` is a primitive in itself, this will only be 32
  bytes when ABI encoded.

  In Substrate, the `SCALE <https://substrate.dev/docs/en/overview/low-level-data-format>`_
  encoding uses 32 bytes for both types.

Storage References
__________________

Parameters, return types, and variables can be declared storage references by adding
``storage`` after the type name. This means that the variable holds a references to a
particular contract storage variable.

.. code-block:: javascript

    contract felix {
        enum Felines { None, Lynx, Felis, Puma, Catopuma };
        Felines[100] group_a;
        Felines[100] group_b;


        function count_pumas(Felines[100] storage cats) private returns (uint32)
    {
            uint32 count = 0;
            uint32 i = 0;

            for (i = 0; i < cats.length; i++) {
                if (cats[i] == Felines.Puma) {
                    ++count;
                }
            }

            return count;
        }

        function all_pumas() public returns (uint32) {
            Felines[100] storage ref = group_a;

            uint32 total = count_pumas(ref);

            ref = group_b;

            total += count_pumas(ref);

            return total;
        }
    }

Functions which have either storage parameter or return types cannot be public; when a function
is called via the ABI encoder/decoder, it is not possible to pass references, just values.
However it is possible to use storage reference variables in public functions, as
demonstrated in function all_pumas().

Expressions
-----------

Solidity resembles the C family of languages. Expressions can have the following operators.

Arithmetic operators
____________________

The binary operators ``-``, ``+``, ``*``, ``/``, ``%``, and ``**`` are supported, and also
in the assignment form ``-=``, ``+=``, ``*=``, ``/=``, and ``%=``. There is a
unary operator ``-``.

.. code-block:: javascript

 	uint32 fahrenheit = celcius * 9 / 5 + 32;

Parentheses can be used too, of course:

.. code-block:: javascript

 	uint32 celcius = (fahrenheit - 32) * 5 / 9;

The assignment operator:

.. code-block:: javascript

 	balance += 10;

The exponation (or power) can be used to multiply a number N times by itself, i.e.
x :superscript:`y`. This can only be done for unsigned types.

.. code-block:: javascript

  uint64 thousand = 1000;
  uint64 billion = thousand ** 3;

.. note::

  No overflow checking is done on the arithmetic operations, just like with the
  Ethereum Foundation Solidity compiler.

Bitwise operators
_________________

The ``|``, ``&``, ``^`` are supported, as are the shift operators ``<<``
and ``>>``. There are also available in the assignment form ``|=``, ``&=``,
``^=``, ``<<=``, and ``>>=``. Lastly there is a unary operator ``~`` to
invert all the bits in a value.

Logical operators
_________________

The logical operators ``||``, ``&&``, and ``!`` are supported. The ``||`` and ``&&``
short-circuit. For example:

.. code-block:: javascript

  bool foo = x > 0 || bar();

bar() will not be called if the left hand expression evaluates to true, i.e. x is greater
than 0. If x is 0, then bar() will be called and the result of the ``||`` will be
the return value of bar(). Similarly, the right hand expressions of ``&&`` will not be
evaluated if the left hand expression evaluates to ``false``; in this case, whatever
ever the outcome of the right hand expression, the ``&&`` will result in ``false``.


.. code-block:: javascript

  bool foo = x > 0 && bar();

Now ``bar()`` will only be called if x *is* greater than 0. If x is 0 then the ``&&``
will result in false, irrespective of what bar() would returns, so bar() is not
called at all. The expression elides execution of the right hand side, which is also
called *short-circuit*.


Ternary operator
________________

The ternary operator ``? :`` is supported:

.. code-block:: javascript

  uint64 abs = foo > 0 ? foo : -foo;


Comparison operators
____________________

It is also possible to compare values. For, this the ``>=``, ``>``, ``==``, ``!=``, ``<``, and ``<=``
is supported. This is useful for conditionals.


The result of a comparison operator can be assigned to a bool. For example:

.. code-block:: javascript

 	bool even = (value % 2) == 0;

It is not allowed to assign an integer to a bool; an explicit comparision is needed to turn it into
a bool.

Increment and Decrement operators
_________________________________

The post-increment and pre-increment operators are implemented like you would expect. So, ``a++``
evaluates to the value of of ``a`` before incrementing, and ``++a`` evaluates to value of ``a``
after incrementing.

Casting
_______

Solidity is strict about the sign of operations, and whether an assignment can truncate a value;
these are errors and Solang will refuse to compile it. You can force the compiler to
accept truncations or differences in sign by adding a cast, but this is best avoided. Often
changing the parameters or return value of a function will avoid the need for casting.

Some examples:

.. code-block:: javascript

  function abs(int bar) public returns (int64) {
      if (bar > 0) {
          return bar;
      } else {
          return -bar;
      }
  }

The compiler will say:

.. code-block:: none

   implicit conversion would truncate from int256 to int64

Now you can work around this by adding a cast to the argument to return ``return int64(bar);``,
however it would be much nicer if the return value matched the argument. Multiple abs() could exists
with overloaded functions, so that there is an ``abs()`` for each type.

It is allowed to cast from a ``bytes`` type to ``int`` or ``uint`` (or vice versa), only if the length
of the type is the same. This requires an explicit cast.

.. code-block:: javascript

  bytes4 selector = "ABCD";
  uint32 selector_as_uint = uint32(selector);

If the length also needs to change, then another cast is needed to adjust the length. Truncation and
extension is different for integers and bytes types. Integers pad zeros on the left when extending,
and truncate on the right. bytes pad on right when extending, and truncate on the left. For example:

.. code-block:: javascript

  bytes4 start = "ABCD";
  uint64 start1 = uint64(uint4(start));
  // first cast to int, then extend as int: start1 = 0x41424344
  uint64 start2 = uint64(bytes8(start));
  // first extend as bytes, then cast to int: start2 = 0x4142434400000000

A similar example for truncation:

.. code-block:: javascript

  uint64 start = 0xdead_cafe;
  bytes4 start1 = bytes4(uint32(start));
  // first truncate as int, then cast: start1 = hex"cafe"
  bytes4 start2 = bytes4(bytes8(start));
  // first cast, then truncate as bytes: start2 = hex"dead"

Since ``byte`` is array of one byte, a conversion from ``byte`` to ``uint8`` requires a cast.

Contract Storage
----------------

Any variables declared at the contract level (so not contained in a function or constructor),
then these will automatically become contract storage. Contract storage is maintained between
calls on-chain. These are declared so:

.. code-block:: javascript

  contract hitcount {
      uint counter = 1;

      function hit() public {
          counters++;
      }

      function count() public view returns (uint) {
          return counter;
      }
  }

The ``counter`` is maintained for each deployed ``hitcount`` contract. When the contract is deployed,
the contract storage is set to 1. The ``= 1`` initializer is not required; when it is not present, it
is initialized to 0, or ``false`` if it is a ``bool``.

Constants
---------

Constants are declared at the contract level just like contract storage variables. However, they
do not use any contract storage and cannot be modified. Assigning a value to a constant is a
compiler error. The variable must have an initializer, which must be a constant expression. It is
not allowed to call functions or read variables in the initializer:

.. code-block:: javascript

  contract ethereum {
      uint constant byzantium_block = 4_370_000;
  }

Constructors
------------

When a contract is deployed, the contract storage is initialized to the initializer values provided,
and any constructor is called. A constructor is not required for a contract. A constructor is defined
like so:

.. code-block:: javascript

  contract mycontract {
      uint foo;

      constructor(uint foo_value) public {
          foo = foo_value;
      }
  }

A constructor does not have a name and may have any number of arguments. If a constructor has arguments,
then when the contract is deployed then those arguments must be supplied.

A constructor must be declared ``public``.

.. note::

  Parity Substrate allows multiple constructors to be defined, which is not true for Hyperledger Burrow
  or ewasm. So, when building for Substrate, multiple constructors can be
  defined as long as their argument list is different (i.e. overloaded).

  When the contract is deployed in the Polkadot UI, the user can select the constructor to be used.

.. note::

  The Ethereum Foundation Solidity compiler allows constructors to be declared ``internal`` if
  for abstract contracts. Since Solang does not support abstract contracts, this is not possible yet.

Declaring Functions
-------------------

Functions can be declared and called as follow:

.. code-block:: javascript

  contact foo {
      uint bound = get_initial_bound();

      /// get_initial_bound is called from the constructor
      function get_initial_bound() private returns (uint value) {
          value = 102;
      }

      /** set bound for get with bound */
      function set_bound(uint _bound) public {
          bound = _bound;
      }

      /// Clamp a value within a bound.
      /// The bound can be set with set_bound().
      function get_with_bound(uint value) view public return (uint) {
          if (value < bound) {
              return value;
          } else {
              return bound;
          }
      }
  }

Function can have any number of arguments. Function arguments may have names;
if they do not have names then they cannot be used in the function body, but they will
be present in the public interface.

The return values may have names as demonstrated in the get_initial_bound() function.
When at least one of the return values has a name, then the return statement is no
longer required at the end of a function body. In stead of returning the values
which are provided in the return statement, the values of the return variables at the end
of the function is returned. It is still possible to explicitly return some values
with a return statement with some values.

Functions which are declared ``public`` will be present in the ABI and are callable
externally. If a function is declared ``private`` then it is not callable externally,
but it can be called from within the contract.

Any DocComment before a function will be include in the ABI. Currently only Substrate
supports documentation in the ABI.

Function overloading
____________________

Multiple functions with the same name can be declared, as long as the arguments are
different in at least one of two ways:

- The number of arguments must be different
- The type of at least one of the arguments is different

A function cannot be overloaded by changing the return types or number of returned
values. Here is an example of an overloaded function:

.. code-block:: javascript

  contract shape {
      int64 bar;

      function abs(int val) public returns (int) {
          if (val >= 0) {
              return val;
          } else {
              return -val;
          }
      }

      function abs(int64 val) public returns (int64) {
          if (val >= 0) {
              return val;
          } else {
              return -val;
          }
      }

      function foo(int64 x) public {
          bar = abs(x);
      }
  }

In the function foo, abs() is called with an ``int64`` so the second implementation
of the function abs() is called.

Function Mutability
___________________

A function which does not access any contract storage, can be declared ``pure``.
Alternatively, if a function only reads contract, but does not write to contract
storage, it can be declared ``view``.

When a function is declared either ``view`` or ``pure``, it can be called without
creating an on-chain transaction, so there is no associated gas cost.

Fallback function
_________________

When a function is called externally, either via an transaction or when one contract
call a function on another contract, the correct function is dispatched based on the
function selector in the raw encoded ABI call data. If no function matches, then the
fallback function is called, if it is defined. If no fallback function is defined then
the call aborts via the ``unreachable`` wasm instruction. A fallback function may not have a name,
any arguments or return values, and must be declared ``external``. Here is an example of
fallback function:

.. code-block:: javascript

  contract test {
      int32 bar;

      function foo(uint32 x) public {
          bar = x;
      }

      function() external {
          bar = 0;
      }
  }

Writing Functions
-----------------

In functions, you can declare variables with the types or an enum. If the name is the same as
an existing function, enum type, or another variable, then the compiler will generate a
warning as the original item is no longer accessible.

.. code-block:: javascript

  contract test {
      uint foo = 102;
      uint bar;

      function foobar() private {
          // AVOID: this shadows the contract storage variable foo
          uint foo = 5;
      }
  }

Scoping rules apply as you would expect, so if you declare a variable in a block, then it is not
accessible outside that block. For example:

.. code-block:: javascript

   function foo() public {
      // new block is introduced with { and ends with }
      {
          uint a;

          a = 102;
      }

      // ERROR: a is out of scope
      uint b = a + 5;
  }

If statement
____________

Conditional execution of a block can be achieved using an ``if (condition) { }`` statement. The
condition must evaluate to a ``bool`` value.

.. code-block:: javascript

  function foo(uint32 n) private {
      if (n > 10) {
          // do something
      }

      // ERROR: unlike C integers can not be used as a condition
      if (n) {
            // ...
      }
  }

The statements enclosed by ``{`` and ``}`` (commonly known as a *block*) are executed only if
the condition evaluates to true.

While statement
_______________

Repeated execution of a block can be achieved using ``while``. It syntax is similar to ``if``,
however the block is repeatedly executed until the condition evaluates to false.
If the condition is not true on first execution, then the loop is never executed:

.. code-block:: javascript

  function foo(uint n) private {
      while (n >= 10) {
          n -= 9;
      }
  }

It is possible to terminate execution of the while statement by using the ``break`` statement.
Execution will continue to next statement in the function. Alternatively, ``continue`` will
cease execution of the block, but repeat the loop if the condition still holds:

.. code-block:: javascript

  function foo(uint n) private {
      while (n >= 10) {
          n--;

          if (n >= 100) {
              // do not execute the if statement below, but loop again
              continue;
          }

          if (bar(n)) {
              // cease execution of this while loop and jump to the "n = 102" statement
              break;
          }
      }

      n = 102;
  }

Do While statement
__________________

A ``do { ... } while (condition);`` statement is much like the ``while (condition) { ... }`` except
that the condition is evaluated after execution the block. This means that the block is executed
at least once, which is not true for ``while`` statements:

.. code-block:: javascript

  function foo(uint n) private {
      do {
          n--;

          if (n >= 100) {
              // do not execute the if statement below, but loop again
              continue;
          }

          if (bar(n)) {
              // cease execution of this while loop and jump to the "n = 102" statement
              break;
          }
      }
      while (n > 10);

      n = 102;
  }

For statements
______________

For loops are like ``while`` loops with added syntaxic sugar. To execute a loop, we often
need to declare a loop variable, set its initial variable, have a loop condition, and then
adjust the loop variable for the next loop iteration.

For example, to loop from 0 to 1000 by steps of 100:

.. code-block:: javascript

  function foo() private {
      for (uint i = 0; i <= 1000; i += 100) {
          // ...
      }
  }

The declaration ``uint i = 0`` can be omitted if no new variable needs to be declared, and
similarly the post increment ``i += 100`` can be omitted if not necessary. The loop condition
must evaluate to a boolean, or it can be omitted completely. If it is ommited the block must
contain a ``break`` or ``return`` statement, else execution will
repeat infinitely (or until all gas is spent):

.. code-block:: javascript

  function foo(uint n) private {
      // all three omitted
      for (;;) {
          // there must be a way out
          if (n == 0) {
              break;
          }
      }
  }
