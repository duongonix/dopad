Name:           dopad
Version:        %{app_version}
Release:        1%{?dist}
Summary:        Lightweight native text editor
License:        LicenseRef-Dopad
Source0:        dopad-%{version}-rootfs.tar.gz
Requires:       glibc
Requires:       libgcc
Requires:       libxkbcommon
Requires:       vulkan-loader
Requires:       fontconfig
Requires:       alsa-lib
Requires:       libwayland-client
Requires:       libX11-xcb

%description
Dopad is a lightweight native text editor. This package includes the desktop
application and the dopad command-line client.

%prep

%build

%install
mkdir -p %{buildroot}
tar -xzf %{SOURCE0} -C %{buildroot}

%files
/usr/bin/dopad
/usr/lib/dopad/dopad-gui
/usr/share/applications/dopad.desktop
/usr/share/icons/hicolor/256x256/apps/dopad.png

%changelog
* Fri Nov 06 2026 Dopad maintainers - %{version}-1
- Package Dopad Desktop and CLI together.
