use std::path::Path;

#[cfg(windows)]
use std::process::Command;

pub(crate) enum PackageLaunch {
    Installer(std::process::Child),
    External,
}

pub(crate) fn supports_automatic_install(path: &Path) -> bool {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    }

    #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
    {
        let _ = path;
        false
    }
}

pub(crate) fn launch_verified_package(path: &Path) -> anyhow::Result<PackageLaunch> {
    #[cfg(target_os = "windows")]
    {
        if supports_automatic_install(path) {
            let mut command = Command::new(path);
            command.args([
                "/VERYSILENT",
                "/CLOSEAPPLICATIONS",
                "/RESTARTAPPLICATIONS",
                "/NORESTART",
            ]);
            crate::process::suppress_console(&mut command);
            let child = command
                .spawn()
                .map_err(|error| anyhow::anyhow!("could not start the Dopad installer: {error}"))?;
            return Ok(PackageLaunch::Installer(child));
        }

        open::that(path).map_err(|error| {
            anyhow::anyhow!("could not open the verified portable Dopad package: {error}")
        })?;
        return Ok(PackageLaunch::External);
    }

    #[cfg(target_os = "macos")]
    {
        open::that(path).map_err(|error| {
            anyhow::anyhow!("could not open the verified Dopad package: {error}")
        })?;
        return Ok(PackageLaunch::External);
    }

    #[cfg(target_os = "linux")]
    {
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("AppImage"))
        {
            let directory = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("downloaded update has no parent directory"))?;
            open::that(directory).map_err(|error| {
                anyhow::anyhow!(
                    "could not open the directory containing the verified AppImage: {error}"
                )
            })?;
        } else {
            open::that(path).map_err(|error| {
                anyhow::anyhow!("could not open the verified Dopad package: {error}")
            })?;
        }
        return Ok(PackageLaunch::External);
    }

    #[allow(unreachable_code)]
    Err(anyhow::anyhow!(
        "update installation is not supported on this platform"
    ))
}

#[cfg(test)]
mod tests {
    use super::supports_automatic_install;
    use std::path::Path;

    #[test]
    fn only_windows_x64_setup_executables_use_automatic_install() {
        let setup = Path::new("dopad-v0.1.2-windows-x86_64-setup.exe");
        let archive = Path::new("dopad-v0.1.2-windows-arm64.zip");

        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            assert!(supports_automatic_install(setup));
            assert!(!supports_automatic_install(archive));
            assert!(!supports_automatic_install(Path::new("update.txt")));
        }

        #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
        {
            assert!(!supports_automatic_install(setup));
            assert!(!supports_automatic_install(archive));
        }
    }
}
