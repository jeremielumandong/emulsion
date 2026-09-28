//! Read-only migration of file references and ratings from recognized Classic catalogs.
//! Adobe's private Develop/history/profile data is deliberately not interpreted.
use crate::{
    IoError, Result,
    creative_library::{AssetKind, Catalog},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportReport {
    pub imported: usize,
    pub collections: usize,
    pub histories: usize,
    pub missing: Vec<PathBuf>,
    pub warnings: Vec<String>,
}
pub fn import(path: &Path, catalog: &mut Catalog) -> Result<ImportReport> {
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("json"))
    {
        return crate::lightroom_bridge::import(path, catalog);
    }

    if path
        .extension()
        .is_none_or(|s| !s.eq_ignore_ascii_case("lrcat"))
    {
        return Err(IoError::Manifest("Choose a .lrcat catalog".into()));
    }
    // No SQL is constructed from user input. -readonly also prevents journal writes.
    let image_columns = sql(path, "PRAGMA table_info(Adobe_images)")?;
    let image_id = if image_columns.iter().any(|v| v["name"] == "id_local") {
        "i.id_local"
    } else {
        "i.rowid"
    };
    let query = format!(
        "PRAGMA trusted_schema=OFF; SELECT {image_id} AS image_id,r.absolutePath AS root,d.pathFromRoot AS folder,f.baseName AS name,f.extension AS extension,COALESCE(i.rating,0) AS rating,COALESCE(i.pick,0) AS pick FROM Adobe_images i JOIN AgLibraryFile f ON i.rootFile=f.id_local JOIN AgLibraryFolder d ON f.folder=d.id_local JOIN AgLibraryRootFolder r ON d.rootFolder=r.id_local LIMIT 10001;"
    );
    use std::{io::Read, process::Stdio};
    let mut child = std::process::Command::new("sqlite3")
        .args(["-readonly", "-json"])
        .arg(path.canonicalize()?)
        .arg(query)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            IoError::Unsupported(format!("Lightroom catalog import requires sqlite3: {e}"))
        })?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 32 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(IoError::Manifest("Catalog import exceeds 32 MiB".into()));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(IoError::Manifest(format!(
            "Unrecognized or busy Lightroom catalog: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    #[derive(Deserialize)]
    struct Row {
        image_id: i64,
        root: String,
        folder: String,
        name: String,
        extension: String,
        rating: i64,
        pick: i64,
    }
    let rows: Vec<Row> = if bytes.is_empty() {
        vec![]
    } else {
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?
    };
    if rows.len() > 10000 {
        return Err(IoError::Manifest(
            "Catalog contains more than 10000 images; export a smaller catalog first".into(),
        ));
    }
    let mut report=ImportReport{imported:0,collections:0,histories:0,missing:vec![],warnings:vec!["Adobe settings use Emulsion rendering. Private/proprietary settings that cannot be translated are reported; rendered Lightroom handoff TIFFs retain Adobe/VSCO appearance.".into()]};
    let mut staged = catalog.clone();
    let mut images = std::collections::BTreeMap::new();
    for row in rows {
        let root = PathBuf::from(&row.root);
        if !root.is_absolute() {
            report.missing.push(
                root.join(&row.folder)
                    .join(format!("{}.{}", row.name, row.extension)),
            );
            continue;
        }
        let file = root.join(row.folder).join(if row.extension.is_empty() {
            row.name
        } else {
            format!("{}.{}", row.name, row.extension)
        });
        if !file.is_file() {
            report.missing.push(file);
            continue;
        }
        if !crate::photo_develop::supported(&file) {
            report
                .warnings
                .push(format!("Unsupported photo: {}", file.display()));
            continue;
        }
        let id = staged.add_asset(file.clone(), AssetKind::Image)?;
        let asset = staged.assets.iter_mut().find(|a| a.id == id).unwrap();
        asset.rating = row.rating.clamp(0, 5) as u8;
        asset.flagged = row.pick > 0;
        asset.rejected = row.pick < 0;
        staged
            .photos
            .fingerprints
            .insert(file.canonicalize()?, crate::raw::source_digest(&file)?);
        images.insert(row.image_id, (id, file));
        report.imported += 1;
    }
    migrate(path, &mut staged, &images, &mut report)?;
    staged.validate()?;
    *catalog = staged;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_recognized_catalog_without_changing_database_or_original() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.png");
        std::fs::write(
            &photo,
            crate::export::png8(1, 1, &[100, 100, 100, 255]).unwrap(),
        )
        .unwrap();
        let db = dir.path().join("test.lrcat");
        let folder = dir.path().to_string_lossy().replace('\'', "''");
        let sql = format!(
            "CREATE TABLE AgLibraryRootFolder(id_local INTEGER,absolutePath TEXT);CREATE TABLE AgLibraryFolder(id_local INTEGER,rootFolder INTEGER,pathFromRoot TEXT);CREATE TABLE AgLibraryFile(id_local INTEGER,folder INTEGER,baseName TEXT,extension TEXT);CREATE TABLE Adobe_images(rootFile INTEGER,rating INTEGER,pick INTEGER);INSERT INTO AgLibraryRootFolder VALUES(1,'{folder}/');INSERT INTO AgLibraryFolder VALUES(2,1,'');INSERT INTO AgLibraryFile VALUES(3,2,'photo','png');INSERT INTO Adobe_images VALUES(3,5,1);"
        );
        let result = std::process::Command::new("sqlite3")
            .arg(&db)
            .arg(sql)
            .status();
        if matches!(&result,Err(e)if e.kind()==std::io::ErrorKind::NotFound) {
            return;
        }
        assert!(result.unwrap().success());
        let before = std::fs::read(&db).unwrap();
        let mut catalog = Catalog::default();
        let report = import(&db, &mut catalog).unwrap();
        assert_eq!(report.imported, 1);
        assert_eq!(catalog.assets[0].rating, 5);
        assert!(catalog.assets[0].flagged);
        assert_eq!(std::fs::read(&db).unwrap(), before);
        assert!(photo.is_file());
    }
}

fn sql(path: &Path, query: &str) -> Result<Vec<serde_json::Value>> {
    use std::{io::Read, process::Stdio};
    let mut child = std::process::Command::new("sqlite3")
        .args(["-readonly", "-json"])
        .arg(path.canonicalize()?)
        .arg(format!("PRAGMA trusted_schema=OFF; {query}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut bytes = vec![];
    child
        .stdout
        .take()
        .unwrap()
        .take((32 << 20) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 32 << 20 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(IoError::Manifest("Catalog query exceeds 32 MiB".into()));
    }
    if !child.wait()?.success() {
        return Err(IoError::Manifest(
            "Unrecognized Lightroom table layout".into(),
        ));
    }
    if bytes.is_empty() {
        Ok(vec![])
    } else {
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))
    }
}
fn migrate(
    path: &Path,
    catalog: &mut Catalog,
    images: &std::collections::BTreeMap<i64, (u64, PathBuf)>,
    report: &mut ImportReport,
) -> Result<()> {
    let tables = sql(path, "SELECT name FROM sqlite_master WHERE type='table'")?;
    let has = |table: &str| tables.iter().any(|v| v["name"] == table);
    if has("AgLibraryCollection") && has("AgLibraryCollectionImage") {
        let result = (|| -> Result<()> {
            let collections = sql(
                path,
                "SELECT id_local,name FROM AgLibraryCollection LIMIT 10001",
            )?;
            if collections.len() > 10000 {
                return Err(IoError::Manifest("Too many Lightroom collections".into()));
            }
            let members = sql(
                path,
                "SELECT collection,image FROM AgLibraryCollectionImage LIMIT 100001",
            )?;
            if members.len() > 100000 {
                return Err(IoError::Manifest("Too many collection members".into()));
            }
            for collection in collections {
                let Some(id) = collection["id_local"].as_i64() else {
                    continue;
                };
                let Some(name) = collection["name"].as_str() else {
                    continue;
                };
                let assets = members
                    .iter()
                    .filter(|m| m["collection"].as_i64() == Some(id))
                    .filter_map(|m| images.get(&m["image"].as_i64()?).map(|v| v.0))
                    .collect();
                catalog.add_collection(name.chars().take(200).collect(), assets)?;
                report.collections += 1;
            }
            Ok(())
        })();
        if let Err(e) = result {
            report.warnings.push(format!("Collections: {e}"));
        }
    }
    let history_table = [
        "AgLibraryImageDevelopHistoryStep",
        "Adobe_imageDevelopHistoryStep",
    ]
    .into_iter()
    .find(|t| has(t));
    let mut history =
        std::collections::BTreeMap::<i64, Vec<(String, emulsion_core::raw::DevelopParams)>>::new();
    if let Some(table) = history_table {
        let columns = sql(path, &format!("PRAGMA table_info({table})"))?;
        let has_col = |name: &str| columns.iter().any(|v| v["name"] == name);
        if let Some(settings) = ["settings", "developSettings", "parameters"]
            .into_iter()
            .find(|n| has_col(n))
            && has_col("image")
        {
            let name = if has_col("name") {
                "name"
            } else {
                "'Imported step'"
            };
            let order = if has_col("dateCreated") {
                "dateCreated"
            } else {
                "rowid"
            };
            let rows = sql(
                path,
                &format!(
                    "SELECT image,{name} AS name,{settings} AS settings FROM {table} ORDER BY {order} LIMIT 100001"
                ),
            )?;
            if rows.len() > 100000 {
                return Err(IoError::Manifest(
                    "Too many Lightroom history entries".into(),
                ));
            }
            for row in rows {
                let Some(id) = row["image"].as_i64() else {
                    continue;
                };
                if !images.contains_key(&id) {
                    continue;
                }
                let Some(text) = row["settings"].as_str() else {
                    continue;
                };
                let states = history.entry(id).or_default();
                let base = states.last().map_or(Default::default(), |v| v.1);
                let parsed = if let Ok(value) = serde_json::from_str(text) {
                    crate::lightroom_presets::from_adobe_settings(&value, base)
                } else {
                    crate::lightroom_presets::from_legacy_settings(text, base)
                };
                match parsed {
                    Ok(p) => {
                        if report.warnings.len() < 100 {
                            report.warnings.extend(p.warnings);
                        }
                        states.push((
                            row["name"].as_str().unwrap_or("Imported step").into(),
                            p.params,
                        ));
                        if states.len() > 100 {
                            states.remove(0);
                        }
                    }
                    Err(e) => {
                        if report.warnings.len() < 100 {
                            report.warnings.push(format!("History step skipped: {e}"));
                        }
                    }
                }
            }
        } else {
            report.warnings.push("Private history encoding is not recognized; use the Lightroom handoff plug-in or adjacent XMP settings".into());
        }
    }
    for (id, (_, source)) in images {
        if crate::raw_settings::sidecar_path(source)?.exists() {
            report
                .warnings
                .push(format!("Existing edits preserved: {}", source.display()));
            continue;
        }
        let mut states = history.remove(id).unwrap_or_default();
        let xmp = source.with_extension("xmp");
        if xmp.is_file() {
            match crate::lightroom_presets::load(
                &xmp,
                states.last().map_or(Default::default(), |p| p.1),
            ) {
                Ok(p) => {
                    report.warnings.extend(p.warnings);
                    states.push(("Current XMP".into(), p.params));
                }
                Err(e) => report.warnings.push(format!("XMP skipped: {e}")),
            }
        }
        if !states.is_empty() {
            crate::raw_settings::import_history(
                source,
                &crate::raw::source_digest(source)?,
                &states,
            )?;
            report.histories += 1;
        }
    }
    Ok(())
}
