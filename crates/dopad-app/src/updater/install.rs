use std::path::Path;

#[cfg(target_os = "windows")]
use std::process::Command;

pub(crate) fn launch_verified_package(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        if path.extension().is_some_and(|extension| extension == "zip") {
            open::that(path).map_err(|error| {
                anyhow::anyhow!("could not open the verified portable Dopad archive: {error}")
            })?;
            return Ok(());
        }
        Command::new(path).spawn().map_err(|error| {
            anyhow::anyhow!("could not launch the verified Dopad installer: {error}")
        })?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        open::that(path).map_err(|error| {
            anyhow::anyhow!("could not open the verified Dopad package: {error}")
        })?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        if path
            .extension()
            .is_some_and(|extension| extension == "AppImage")
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
            open::that(path)
                .map_err(|error| anyhow::anyhow!("could not open the verified package: {error}"))?;
        }
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(anyhow::anyhow!(
        "in-app update installation is not supported on this platform"
    ))
}
