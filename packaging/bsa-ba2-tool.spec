Name:           bsa-ba2-tool
Version:        0.0.1
Release:        1%{?dist}
Summary:        CLI tool for reading and writing Bethesda BSA/BA2 archives

License:        MIT
URL:            https://github.com/phantomcortex/Rust-BSA-BA2-Handler
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc-c++
BuildRequires:  fontconfig-devel
BuildRequires:  libxcb-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  wayland-devel
BuildRequires:  mesa-libGL-devel
BuildRequires:  dbus-devel

%description
bsa-ba2-tool is a command-line utility for reading and writing Bethesda Archive
(BSA) and Bethesda Archive 2 (BA2) files used by Creation Engine games,
including Morrowind, Oblivion, Skyrim, Fallout 4, and Starfield.

Used by file-roller as a backend to provide native BSA/BA2 archive support.

%prep
%autosetup

%build
cargo build --release --bin bsa-ba2-tool

%install
install -Dm755 target/release/bsa-ba2-tool %{buildroot}%{_bindir}/bsa-ba2-tool

%files
%{_bindir}/bsa-ba2-tool

%changelog
* Thu Jun 18 2026 phantomcortex <phantom.github@proton.me> - 0.0.1-1
- Initial RPM packaging for DistinctionOS
