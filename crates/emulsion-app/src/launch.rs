//! File-manager launches preserve every path, including non-UTF-8 names.
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, PartialEq)]
pub enum Launch {
    Help,
    Version,
    Mcp,
    Gui { files: Vec<PathBuf>, edit: bool },
}
pub fn parse(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<Launch> {
    let mut files = Vec::new();
    let mut edit = false;
    let mut literal = false;
    for arg in args {
        if !literal {
            match arg.to_str() {
                Some("--") => {
                    literal = true;
                    continue;
                }
                Some("--edit") => {
                    edit = true;
                    continue;
                }
                Some("--help" | "-h") if files.is_empty() => return Ok(Launch::Help),
                Some("--version" | "-V") if files.is_empty() => return Ok(Launch::Version),
                Some("mcp-serve") if files.is_empty() => return Ok(Launch::Mcp),
                Some(option) if option.starts_with('-') => {
                    anyhow::bail!("unknown option: {option}")
                }
                _ => {}
            }
        }
        files.push(arg.into());
    }
    Ok(Launch::Gui { files, edit })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_manager_paths_and_explicit_editor_are_preserved() {
        assert_eq!(
            parse(["one image.png", "two.jpg"].map(Into::into)).unwrap(),
            Launch::Gui {
                files: vec!["one image.png".into(), "two.jpg".into()],
                edit: false
            }
        );
        assert_eq!(
            parse(["--edit", "--", "-photo.png"].map(Into::into)).unwrap(),
            Launch::Gui {
                files: vec!["-photo.png".into()],
                edit: true
            }
        );
        assert!(parse(["--unknown".into()]).is_err());
        assert_eq!(parse(["mcp-serve".into()]).unwrap(), Launch::Mcp);
    }
    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_not_lossily_rewritten() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"photo-\xff.png".to_vec());
        assert_eq!(
            parse([path.clone()]).unwrap(),
            Launch::Gui {
                files: vec![path.into()],
                edit: false
            }
        );
    }
}
