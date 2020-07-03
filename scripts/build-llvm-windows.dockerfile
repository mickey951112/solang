# escape=`

# Use the latest Windows Server Core image with .NET Framework 4.8.
FROM mcr.microsoft.com/windows/servercore:ltsc2019

SHELL [ "powershell", "-Command", "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'Continue'; $verbosePreference='Continue';"]

# Download the Build Tools bootstrapper.
ADD https://aka.ms/vs/16/release/vs_buildtools.exe C:\TEMP\vs_buildtools.exe

# Install Visual Studio Build Tools
RUN C:\TEMP\vs_buildtools.exe --quiet --wait --norestart --nocache `
    --installPath C:\BuildTools `
    --add Microsoft.VisualStudio.Component.VC.CMake.Project `
    --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    --add Microsoft.VisualStudio.Component.VC.ATL `
    --add Microsoft.VisualStudio.Component.Windows10SDK.16299 `
    --remove Microsoft.VisualStudio.Component.Windows10SDK.10240 `
    --remove Microsoft.VisualStudio.Component.Windows10SDK.10586 `
    --remove Microsoft.VisualStudio.Component.Windows10SDK.14393 `
    --remove Microsoft.VisualStudio.Component.Windows81SDK

# Rust
ADD https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe C:\TEMP\rustup-init.exe

RUN C:\TEMP\rustup-init.exe -y

# Git
ADD https://github.com/git-for-windows/git/releases/download/v2.12.2.windows.2/MinGit-2.12.2.2-64-bit.zip C:\TEMP\MinGit.zip

RUN Expand-Archive C:\TEMP\MinGit.zip -DestinationPath c:\MinGit

# LLVM Build requires Python
# Newer versions than v3.5.4 fail due to https://github.com/microsoft/vcpkg/issues/6988
ADD https://www.python.org/ftp/python/3.5.4/python-3.5.4-embed-amd64.zip C:\TEMP\python-3.5.4-embed-amd64.zip

RUN Expand-Archive C:\TEMP\python-3.5.4-embed-amd64.zip -DestinationPath c:\Python

# PowerShell community extensions needed for Invoke-BatchFile
RUN Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force ; `
	Install-Module -name Pscx -Scope CurrentUser -Force -AllowClobber

# Invoke-BatchFile retains the environment after executing so we can set it up more permanently
RUN Invoke-BatchFile C:\BuildTools\vc\Auxiliary\Build\vcvars64.bat ; `
	$path = $env:path + ';c:\MinGit\cmd;C:\Users\ContainerAdministrator\.cargo\bin;C:\llvm80\bin;C:\Python' ; `
	Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\' -Name Path -Value $path ; `
	Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\' -Name LIB -Value $env:LIB ; `
	Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\' -Name INCLUDE -Value $env:INCLUDE ; `
	Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\' -Name LIBPATH -Value $env:LIBPATH ;

RUN git clone -b release/8.x git://github.com/llvm/llvm-project

WORKDIR llvm-project

# Stop cmake from re-generating build system ad infinitum and fix missing include
RUN Add-Content llvm\CMakeLists.txt 'set(CMAKE_SUPPRESS_REGENERATION 1)' ; `
	$header = Get-Content .\llvm\include\llvm\Demangle\MicrosoftDemangleNodes.h ; `
	$header[8] = '#include <string>' ; `
	$header | Set-Content .\llvm\include\llvm\Demangle\MicrosoftDemangleNodes.h

# All llvm targets should be enabled or inkwell refused to link
RUN cmake -G Ninja -DLLVM_ENABLE_ASSERTIONS=On -DLLVM_ENABLE_PROJECTS=clang `
	-DCMAKE_BUILD_TYPE=MinSizeRel -DCMAKE_INSTALL_PREFIX=C:/llvm80 `
	-B build llvm
RUN cmake --build build --target install

WORKDIR \

RUN Compress-Archive -Path C:\llvm80 -DestinationPath C:\llvm

RUN Remove-Item -Path llvm-project,C:\TEMP -Recurse -Force
