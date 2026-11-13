# gtw RPM spec — binary packaging of the prebuilt release artifact.
%global debug_package %{nil}

Name: gtw
Version: %{gtw_version}
Release: 1
Summary: Wikipedia in the terminal with cached reading and rich media
License: GPL-3.0-only
URL: https://github.com/craftty-org/gtw
Requires: alsa-lib

%description
gtw is a keyboard- and mouse-driven Wikipedia reader for the terminal. It
renders articles with responsive wrapping, syntax highlighting, inline images,
audio and video previews, LaTeX math, a compressed offline cache, bookmarks,
history, and reading lists.

%install
cp -a %{gtw_staging}/. %{buildroot}/

%files
%{_bindir}/gtw
%{_mandir}/man1/gtw.1*
%{_datadir}/licenses/gtw/LICENSE
%{_datadir}/bash-completion/completions/gtw
%{_datadir}/zsh/site-functions/_gtw
%{_datadir}/fish/vendor_completions.d/gtw.fish
%{_datadir}/elvish/lib/gtw.elv
%{_datadir}/powershell/Modules/gtw.ps1

%changelog
* %{gtw_changelog_date} prjctimg <prjctimg@outlook.com> - %{gtw_version}-1
- Initial binary package release
