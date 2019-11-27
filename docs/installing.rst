Installing Solang
=================

The solang compiler is a single binary. It can be installed in many different
ways, so please pick whatever method suits your needs.

Using hyperledgerlabs/solang docker hub images
----------------------------------------------

Simply pull the latest docker image using::

	docker pull hyperledgerlabs/solang

And if you are using podman::

	podman image pull hyperlederlabs/solang

The solang binary is in ``/usr/bin/solang`` in this image. The latest tag
gets updated each time there is a commit to the master branch of the solang
git repository.

Build solang from source using Dockerfile
-----------------------------------------

First clone the git repo using::

  git clone https://github.com/hyperledger-labs/solang

Then you can build the image using::

	docker build .

Alternatively this will work with podman too::

	podman image build .

Building solang from source using cargo
---------------------------------------

solang is listed on `crates.io <https://crates.io/crates/solang>`_. Only
releases are pushed to cargo. Do install using cargo::

	cargo install solang

You will need the llvm libraries for this to work, see 
`Getting the right version of LLVM`_.

Building solang from source
---------------------------
In order to build solang from source, you will need rust 1.33.0 or higher,
and llvm version 8 or higher with the WebAssembly target enabled.

So see if you have the correct version of rust, simply execute::

  rustc --version

If you do not have the correct version of rust installed, go to `rustup <https://rustup.rs/>`_.

To make sure you have the correct version of the llvm libraries installed, first run::

  llvm-config --version

The output should be 8.0 or higher. Then check if the WebAssembly target is enabled by running::

  llc --version

You should see wasm32 listed under the target. Lastly check that the static libraries are installed::

  llvm-version --link-static --libs

If there is no output, there are no static llvm libraries and building will fail.

After making sure llvm and rust are installed, just run::

  cargo build --release

The executable will be in ``target/release/solang``.

Getting the right version of LLVM
---------------------------------
If you did not have the llvm libraries installed then you can either install
your systems llvm packages or compile your own. Compiling your own is helpful
if you want to do solang development.

Any version from llvm 8.0 will do. Note that you will also need clang; the
Solidity standard library is written in C, and is compiled to wasm by
clang. For this to work, the version of clang **must** be the same as the
version of llvm.

Installing LLVM on Ubuntu
_________________________

You will need ubuntu 18.04 with backports or later. Just run::

	sudo apt install curl llvm clang git zlib1g-dev cargo

Installing LLVM on Fedora
_________________________

You will need Fedora 30 or later. Running the following::

	sudo dnf install cargo llvm8.0-static llvm8.0-devel zlib-devel clang libffi-devel

Installing LLVM from source
___________________________

If your system does not come with llvm, then you have to build your own.
Building your own llvm libraries does not interfere with any llvm libraries
installed by your distribution.

The llvm project is a large code base so it will take some time to build.

If you are planning to do development on solang itself, then having your
own built llvm is very helpful. The distributions build llvm without 
assertions enabled. These assertions check that the IR that solang builds
is correct. If the IR is not correct, then faults might happen when llvm
runs compiler passes on the IR and the stack trace will not be useful
to debug this problem.

The llvm project itself has a guide to `installing from source <http://www.llvm.org/docs/CMake.html>`_ which you may need to consult. 
First if all clone the llvm repository::

	git clone https://github.com/llvm/llvm-project
	cd llvm-project

Now switch to the 8.0 release branch::

	git checkout -b release_8.x origin/release/8.x

Ensure that clang will built::

	ln -s clang llvm/tools/clang

Create a directory where the build and intermediate files will be stored::

	mkdir build
	cd build

Now run cmake to create the makefiles. Replace the *installdir* argument to ``CMAKE_INSTALL_PREFIX`` with with a directory where you would like to have llvm installed, and then run the build::

	cmake -G Ninja -DLLVM_TARGETS_TO_BUILD=WebAssembly -DLLVM_ENABLE_ASSERTIONS=On -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_INSTALL_PREFIX=installdir ../llvm
	cmake --build . --target install

Once the build has succeeded, the *installdir*/bin has to be added to your path so the solang build can find this llvm config::

	export PATH=*installdir*/bin:$PATH

