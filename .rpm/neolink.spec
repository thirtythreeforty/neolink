%define __spec_install_post %{nil}
%define __os_install_post %{_dbpath}/brp-compress
%define debug_package %{nil}

Name: neolink
Summary: A standards-compliant bridge to Reolink IP cameras
Version: @@VERSION@@
Release: @@RELEASE@@%{?dist}
License: AGPLv3+
Group: Applications/System
Source0: %{name}-%{version}.tar.gz
Requires: gstreamer1 gstreamer1-plugins-base gstreamer1-rtsp-server gstreamer1-plugins-good gstreamer1-plugins-bad-free

BuildRoot: %{_tmppath}/%{name}-%{version}-%{release}-root

%description
%{summary}

%prep
%setup -q

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a * %{buildroot}

%clean
rm -rf %{buildroot}

%files
%defattr(-,root,root,-)
%{_bindir}/*
