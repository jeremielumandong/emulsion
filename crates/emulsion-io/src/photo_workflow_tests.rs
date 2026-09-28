use super::*;
use emulsion_core::{
    Document, Node,
    raw::{DevelopParams, PointCurve},
};
use emulsion_raster::{Raster, composite::flatten};
use std::sync::Arc;
fn photo(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("photo.png");
    std::fs::write(
        &p,
        export::png8(4, 4, &[80, 120, 180, 255].repeat(16)).unwrap(),
    )
    .unwrap();
    p
}
#[test]
fn virtual_relink_preserves_independent_edit_history() {
    let dir = tempfile::tempdir().unwrap();
    let original = photo(dir.path());
    let params = DevelopParams {
        exposure: 0.8,
        ..Default::default()
    };
    let copy =
        photo_develop::create_virtual(&original, params, &dir.path().join("copies")).unwrap();
    let source = photo_develop::PhotoSource::load(&copy).unwrap();
    raw_settings::save_snapshot(&copy, &source.source_sha256, "Copy", params).unwrap();
    drop(source);
    let mut catalog = creative_library::Catalog::default();
    photo_catalog::import(&mut catalog, &[original.clone(), copy.clone()], false).unwrap();
    let moved = dir.path().join("moved.png");
    std::fs::rename(&original, &moved).unwrap();
    photo_catalog::relink(&mut catalog, &original, &moved).unwrap();
    assert_eq!(photo_develop::original_path(&copy).unwrap(), moved);
    let source = photo_develop::PhotoSource::load(&copy).unwrap();
    assert_eq!(
        raw_settings::adjacent_settings(&copy, &source.source_sha256).unwrap(),
        params
    );
    assert_eq!(
        raw_settings::photo_history(&copy, &source.source_sha256)
            .unwrap()
            .1["Copy"],
        params
    );
}
#[test]
fn portable_backup_restores_originals_virtuals_and_recipes_to_new_paths() {
    let dir = tempfile::tempdir().unwrap();
    let original = photo(dir.path());
    let params = DevelopParams {
        exposure: 0.6,
        ..Default::default()
    };
    let copy =
        photo_develop::create_virtual(&original, params, &dir.path().join("copies")).unwrap();
    let mut catalog = creative_library::Catalog::default();
    photo_catalog::import(&mut catalog, &[original.clone(), copy], false).unwrap();
    let backup = dir.path().join("backup.emulibrary");
    photo_backup::save(&catalog, &backup).unwrap();
    assert!(photo_backup::save(&catalog, &backup).is_err());
    let restored = photo_backup::restore(&backup, &dir.path().join("restored")).unwrap();
    assert_eq!(restored.assets.len(), 2);
    let r = restored
        .assets
        .iter()
        .find(|a| photo_develop::is_virtual(&a.path))
        .unwrap();
    assert_ne!(photo_develop::original_path(&r.path).unwrap(), original);
    let source = photo_develop::PhotoSource::load(&r.path).unwrap();
    assert_eq!(
        raw_settings::adjacent_settings(&r.path, &source.source_sha256).unwrap(),
        params
    );
    assert_eq!(
        std::fs::read(&original).unwrap(),
        std::fs::read(photo_develop::original_path(&r.path).unwrap()).unwrap()
    );
}
#[test]
fn channel_curves_preserve_control_points_and_only_change_target_channel() {
    let mut params = DevelopParams::default();
    params.point_curves[1] = PointCurve::try_from(vec![[0., 1.], [0.2, 0.6], [1., 0.]]).unwrap();
    let json = serde_json::to_vec(&params).unwrap();
    assert_eq!(
        serde_json::from_slice::<DevelopParams>(&json).unwrap(),
        params
    );
    let source = Raster::solid(4, 4, [0.1, 0.2, 0.3, 1.]);
    let output = raw::develop_raster(&source, &params).unwrap();
    assert_ne!(output.get(2, 2)[0], source.get(2, 2)[0]);
    assert_eq!(output.get(2, 2)[1..], source.get(2, 2)[1..]);
    assert!(PointCurve::try_from(vec![[0.5, 0.], [0.2, 1.]]).is_err());
}
#[test]
fn metadata_privacy_survives_every_library_output_format() {
    use ::exif::{Field, In, Tag, Value};
    let dir = tempfile::tempdir().unwrap();
    let fields = vec![
        Field {
            tag: Tag::Make,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"Test camera".to_vec()]),
        },
        Field {
            tag: Tag::GPSLatitudeRef,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"N".to_vec()]),
        },
        Field {
            tag: Tag::BodySerialNumber,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"secret".to_vec()]),
        },
        Field {
            tag: Tag::Orientation,
            ifd_num: In::PRIMARY,
            value: Value::Short(vec![6]),
        },
    ];
    let mut writer = ::exif::experimental::Writer::new();
    for f in &fields {
        writer.push_field(f);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    writer.write(&mut bytes, true).unwrap();
    let mut doc = Document::new(4, 4);
    doc.nodes.push(Node::raster(
        1,
        "Photo",
        Arc::new(Raster::solid(4, 4, [0.2, 0.3, 0.4, 1.])),
        Default::default(),
    ));
    doc.next_id = 2;
    let source = dir.path().join("source.png");
    export::export_with_exif(
        &doc,
        &source,
        export::ExportOptions::for_doc(&doc),
        Some(bytes.get_ref()),
    )
    .unwrap();
    for policy in [
        photo_metadata::Policy::None,
        photo_metadata::Policy::Camera,
        photo_metadata::Policy::CameraAndLocation,
    ] {
        let meta = photo_metadata::build(&source, policy).unwrap();
        for ext in ["png", "jpg", "webp", "tif"] {
            let path = dir.path().join(format!("out.{ext}"));
            export::export_with_exif(
                &doc,
                &path,
                export::ExportOptions::for_doc(&doc),
                meta.as_deref(),
            )
            .unwrap();
            let mut f = std::io::BufReader::new(std::fs::File::open(path).unwrap());
            let exif = ::exif::Reader::new().read_from_container(&mut f);
            if policy == photo_metadata::Policy::None {
                if let Ok(exif) = exif {
                    for tag in [Tag::Make, Tag::GPSLatitudeRef, Tag::BodySerialNumber] {
                        assert!(exif.get_field(tag, In::PRIMARY).is_none());
                    }
                }
            } else {
                let exif = exif.unwrap();
                assert!(exif.get_field(Tag::Make, In::PRIMARY).is_some());
                assert!(exif.get_field(Tag::BodySerialNumber, In::PRIMARY).is_none());
                assert_eq!(
                    exif.get_field(Tag::GPSLatitudeRef, In::PRIMARY).is_some(),
                    policy == photo_metadata::Policy::CameraAndLocation
                );
                assert_eq!(
                    exif.get_field(Tag::Orientation, In::PRIMARY)
                        .unwrap()
                        .value
                        .get_uint(0),
                    Some(1)
                );
            }
        }
    }
    assert_eq!(flatten(&doc.composite_tree(), 0).width(), 4);
}
#[test]
fn automatic_geometry_levels_structured_image_and_rejects_blank() {
    let w = 240;
    let h = 200;
    let (s, c) = 7f32.to_radians().sin_cos();
    let mut pixels = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 - w as f32 / 2., y as f32 - h as f32 / 2.);
            let (xx, yy) = (c * dx + s * dy, -s * dx + c * dy);
            if (xx.abs() - 60.).abs() < 2. || (yy.abs() - 55.).abs() < 2. {
                pixels[((y * w + x) * 4) as usize..((y * w + x) * 4 + 3) as usize].fill(0);
            }
        }
    }
    let source = Raster::from_srgba8(w, h, &pixels);
    let result = photo_geometry::automatic(&source, Default::default()).unwrap();
    assert!((result.straighten.abs() - 7.).abs() < 2., "{result:?}");
    assert!(
        photo_geometry::automatic(&Raster::solid(64, 64, [0.5; 4]), Default::default()).is_err()
    );
}
#[test]
fn lightroom_collection_history_and_handoff_migration_preserve_existing_edits() {
    let dir = tempfile::tempdir().unwrap();
    let original = photo(dir.path());
    let db = dir.path().join("catalog.lrcat");
    let folder = dir.path().to_string_lossy().replace('\'', "''");
    let sql = format!(
        "CREATE TABLE AgLibraryRootFolder(id_local INTEGER,absolutePath TEXT);CREATE TABLE AgLibraryFolder(id_local INTEGER,rootFolder INTEGER,pathFromRoot TEXT);CREATE TABLE AgLibraryFile(id_local INTEGER,folder INTEGER,baseName TEXT,extension TEXT);CREATE TABLE Adobe_images(id_local INTEGER,rootFile INTEGER,rating INTEGER,pick INTEGER);CREATE TABLE AgLibraryCollection(id_local INTEGER,name TEXT);CREATE TABLE AgLibraryCollectionImage(collection INTEGER,image INTEGER);CREATE TABLE AgLibraryImageDevelopHistoryStep(image INTEGER,name TEXT,settings TEXT,dateCreated INTEGER);INSERT INTO AgLibraryRootFolder VALUES(1,'{folder}/');INSERT INTO AgLibraryFolder VALUES(2,1,'');INSERT INTO AgLibraryFile VALUES(3,2,'photo','png');INSERT INTO Adobe_images VALUES(4,3,5,1);INSERT INTO AgLibraryCollection VALUES(5,'Travel');INSERT INTO AgLibraryCollectionImage VALUES(5,4);INSERT INTO AgLibraryImageDevelopHistoryStep VALUES(4,'Exposure','{{\"Exposure2012\":0.5}}',1);INSERT INTO AgLibraryImageDevelopHistoryStep VALUES(4,'Color','{{\"Saturation\":-20}}',2);"
    );
    assert!(
        std::process::Command::new("sqlite3")
            .arg(&db)
            .arg(sql)
            .status()
            .unwrap()
            .success()
    );
    let mut catalog = creative_library::Catalog::default();
    let report = lightroom_catalog::import(&db, &mut catalog).unwrap();
    assert_eq!(
        (report.imported, report.collections, report.histories),
        (1, 1, 1)
    );
    let digest = raw::source_digest(&original).unwrap();
    let p = raw_settings::adjacent_settings(&original, &digest).unwrap();
    assert_eq!(p.exposure, 0.5);
    assert_eq!(p.saturation, -0.2);
    assert_eq!(
        raw_settings::photo_history(&original, &digest)
            .unwrap()
            .0
            .len(),
        1
    );
    let before = std::fs::read(raw_settings::sidecar_path(&original).unwrap()).unwrap();
    lightroom_catalog::import(&db, &mut catalog).unwrap();
    assert_eq!(
        std::fs::read(raw_settings::sidecar_path(&original).unwrap()).unwrap(),
        before
    );
    let handoff = dir.path().join("handoff.emulr.json");
    std::fs::write(&handoff,serde_json::to_vec(&serde_json::json!({"format":"emulsion-lightroom-handoff","version":1,"photos":[{"original":"photo.png","rendered":null,"rating":4,"collections":["Handoff"],"settings":{"Exposure2012":2.}}]})).unwrap()).unwrap();
    let result = lightroom_bridge::import(&handoff, &mut catalog).unwrap();
    assert_eq!(result.imported, 1);
    assert_eq!(
        raw_settings::adjacent_settings(&original, &digest).unwrap(),
        p
    );
}
