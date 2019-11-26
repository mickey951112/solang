Solang Solidity Compiler
========================

Welcome the solang Solidity compiler. Using solang, you can compile 
smart contracts written in `Solidity <https://en.wikipedia.org/wiki/Solidity>`_ for `Substrate <https://substrate.dev/>`_ or `Hyperledger Burrow <https://github.com/hyperledger/burrow>`_. It uses the
`llvm <https://www.llvm.org/>`_ compiler framework to produce WebAssembly
(wasm). As result, the output is highly optimized, which saves you in gas costs.

The Solidity language support is not fully compatible with the Ethereum
EVM Solidity compiler. Where differences exists, this is noted in the
documentation. In our flavour of solidity, there are also novel features
like the condition argument to the ``if`` statement does not need
parentheses, like in Go or rust.

Many language features are not implemented yet. Anything which is documented
is supported, though.

.. toctree::
   :maxdepth: 3
   :caption: Contents:

   installing
   running
   language

Indices and tables
==================

* :ref:`genindex`
* :ref:`modindex`
* :ref:`search`
