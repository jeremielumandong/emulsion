//! Procedurally generated DNG. No third-party photographs or camera claims.
use rawler::formats::tiff::{DirectoryWriter, Rational, SRational, TiffWriter, Value};
use std::path::Path;

pub fn write_dng(path: &Path) {
    write_dng_variant(path, false, 1);
}

pub fn write_dng_variant(path: &Path, xtrans: bool, orientation: u16) {
    let (width, height) = (36usize, 24usize);
    let pattern: &[u8] = if xtrans {
        &[
            1, 2, 1, 1, 0, 1, 0, 1, 0, 2, 1, 2, 1, 2, 1, 1, 0, 1, 1, 0, 1, 1, 2, 1, 2, 1, 2, 0, 1,
            0, 1, 0, 1, 1, 2, 1,
        ]
    } else {
        &[0, 1, 1, 2]
    };
    let repeat = if xtrans { 6 } else { 2 };
    let pixels: Vec<u16> = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                let channel = pattern[(y % repeat) * repeat + x % repeat];
                let neutral = (0.15 + 0.5 * x as f32 / (width - 1) as f32) * 4000.0;
                64 + (neutral / [2.0, 1.0, 1.5][channel as usize]) as u16
            })
        })
        .collect();
    let mut writer = TiffWriter::new(std::fs::File::create(path).unwrap()).unwrap();
    let offset = writer.write_data_u16_le(&pixels).unwrap();
    let mut ifd = DirectoryWriter::new();
    ifd.add_tag(254u16, 0u32);
    ifd.add_tag(256u16, width as u32);
    ifd.add_tag(257u16, height as u32);
    ifd.add_tag(258u16, 16u16);
    ifd.add_tag(259u16, 1u16);
    ifd.add_tag(262u16, 32803u16);
    ifd.add_tag(271u16, "Emulsion Synthetic");
    ifd.add_tag(
        272u16,
        if xtrans {
            "Synthetic X-Trans"
        } else {
            "Synthetic Bayer"
        },
    );
    ifd.add_tag(273u16, offset);
    ifd.add_tag(274u16, orientation);
    ifd.add_tag(277u16, 1u16);
    ifd.add_tag(278u16, height as u32);
    ifd.add_tag(279u16, (pixels.len() * 2) as u32);
    ifd.add_tag(33421u16, [repeat as u16, repeat as u16]);
    ifd.add_tag(33422u16, Value::Byte(pattern.to_vec()));
    ifd.add_tag(50706u16, Value::Byte(vec![1, 4, 0, 0]));
    ifd.add_tag(50707u16, Value::Byte(vec![1, 1, 0, 0]));
    ifd.add_tag(50708u16, "Emulsion synthetic regression fixture");
    ifd.add_tag(50713u16, [1u16, 1]);
    ifd.add_tag(50714u16, 64u16);
    ifd.add_tag(50717u16, 4064u32);
    let matrix = rawler::imgop::xyz::XYZ_TO_SRGB_D65;
    ifd.add_tag(
        50721u16,
        Value::SRational(
            matrix
                .into_iter()
                .flatten()
                .map(|v| SRational::new((v * 1_000_000.0).round() as i32, 1_000_000))
                .collect(),
        ),
    );
    ifd.add_tag(
        50728u16,
        Value::Rational(vec![
            Rational::new(1, 2),
            Rational::new(1, 1),
            Rational::new(2, 3),
        ]),
    );
    ifd.add_tag(50778u16, 21u16);
    writer.build(ifd).unwrap();
}
