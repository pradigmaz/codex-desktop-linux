Name:           __PACKAGE_NAME__
Version:        __RPM_VERSION__
Release:        __RPM_RELEASE__%{?dist}
Summary:        Codex Desktop for Linux
License:        Proprietary
ExclusiveArch:  __ARCH__

Requires:       nodejs, npm, python3, p7zip, curl, unzip, gcc-c++, make
Requires:       alsa-lib, at-spi2-atk, atk, glib2, gtk3, libdrm
Requires:       nspr, nss, pango, libstdc++, libX11, libxcb
Requires:       libXcomposite, libXdamage, libXext, libXfixes, libxkbcommon, libXrandr
Requires:       mesa-libgbm

%description
Community-built Linux package for Codex Desktop generated from the macOS DMG.
Requires the Codex CLI to be available in PATH or configured via CODEX_CLI_PATH.
Local auto-updates rebuild a Linux package from the upstream Codex.dmg and therefore
require the local packaging toolchain listed in Requires.

%install
# Files are staged by build-rpm.sh outside of BUILDROOT and copied here.
mkdir -p %{buildroot}
cp -a "__RPM_STAGING_DIR__/." "%{buildroot}/"

%files
%defattr(-,root,root,-)
/opt/__PACKAGE_NAME__/
/usr/bin/__PACKAGE_NAME__
/usr/bin/codex-update-manager
/usr/lib/systemd/user/codex-update-manager.service
/usr/share/applications/__PACKAGE_NAME__.desktop
/usr/share/icons/hicolor/256x256/apps/__PACKAGE_NAME__.png

%post
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
fi

%preun
if command -v runuser >/dev/null 2>&1 && command -v systemctl >/dev/null 2>&1; then
    cleanup_user_service() {
        action="$1"
        for runtime_dir in /run/user/*; do
            [ -d "$runtime_dir" ] || continue

            uid="$(basename "$runtime_dir")"
            case "$uid" in
                ''|*[!0-9]*|0)
                    continue
                    ;;
            esac

            bus="$runtime_dir/bus"
            [ -S "$bus" ] || continue

            user_name="$(getent passwd "$uid" | cut -d: -f1 || true)"
            [ -n "$user_name" ] || continue

            runuser -u "$user_name" -- env \
                XDG_RUNTIME_DIR="$runtime_dir" \
                DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" \
                systemctl --user "$action" codex-update-manager.service >/dev/null 2>&1 || true

            runuser -u "$user_name" -- env \
                XDG_RUNTIME_DIR="$runtime_dir" \
                DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" \
                systemctl --user daemon-reload >/dev/null 2>&1 || true
        done
    }

    cleanup_user_service stop
    if [ $1 -eq 0 ]; then
        cleanup_user_service disable
    fi
fi

%postun
if command -v runuser >/dev/null 2>&1 && command -v systemctl >/dev/null 2>&1; then
    cleanup_user_service() {
        for runtime_dir in /run/user/*; do
            [ -d "$runtime_dir" ] || continue

            uid="$(basename "$runtime_dir")"
            case "$uid" in
                ''|*[!0-9]*|0)
                    continue
                    ;;
            esac

            bus="$runtime_dir/bus"
            [ -S "$bus" ] || continue

            user_name="$(getent passwd "$uid" | cut -d: -f1 || true)"
            [ -n "$user_name" ] || continue

            runuser -u "$user_name" -- env \
                XDG_RUNTIME_DIR="$runtime_dir" \
                DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" \
                systemctl --user daemon-reload >/dev/null 2>&1 || true
        done
    }

    cleanup_user_service daemon-reload
fi

%changelog
* Thu Jan 01 2026 Codex Desktop Linux Maintainers <maintainers@codex-desktop-linux>
- Initial RPM package
