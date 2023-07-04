use anyhow::anyhow;
use bytes::Bytes;
use image::codecs::jpeg::JpegEncoder;
use image::{ImageBuffer, Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use rusttype::{Font, Scale};

use crate::api_requester::{Album, CLIENT_NOCACHE};

const FONT_PATH: &str = "NotoSansCJKtc-Medium.ttf";
const FONT_SIZE: f32 = 24.0;
const TILE_PX: u32 = 300;
pub const MAX_SIZE: u32 = 7;
pub const MIN_SIZE: u32 = 1;

async fn fetch_album_arts(albums: &[&Album]) -> Vec<Result<Bytes, anyhow::Error>> {
    let mut handles = Vec::new();
    albums
        .iter()
        .map(|album| {
            CLIENT_NOCACHE
                .get(album.album_art_url.as_ref().unwrap())
                .send()
        })
        .for_each(|fut| {
            let handle = tokio::spawn(async move {
                let resp = fut.await;
                match resp {
                    Ok(resp) => Ok(resp.bytes().await.unwrap_or_default()),
                    Err(e) => Err(anyhow!(e)),
                }
            });
            handles.push(handle);
        });

    let mut bytes_results: Vec<Result<Bytes, anyhow::Error>> = Vec::new();

    for handle in handles {
        bytes_results.push(handle.await.unwrap());
    }

    bytes_results
}

pub async fn create_collage(
    albums: &[Album],
    size: u32,
    text: bool,
) -> Result<Vec<u8>, anyhow::Error> {
    let collage_size: u32 = TILE_PX * size;

    let mut collage = ImageBuffer::from_pixel(collage_size, collage_size, Rgba([0, 0, 0, 255]));
    let font_data = std::fs::read(FONT_PATH).ok().unwrap();
    let font = Font::try_from_vec(font_data).unwrap();

    let albums = albums
        .iter()
        .filter(|x| x.album_art_url.is_some())
        .take((size * size).try_into().unwrap())
        .collect::<Vec<_>>();

    let tiles_bytes_vec = fetch_album_arts(&albums).await;

    for (i, album) in albums.iter().enumerate() {
        let tiles_bytes = &tiles_bytes_vec[i];

        let row = i as u32 / size;
        let col = i as u32 % size;
        let tile_x = col * TILE_PX;
        let tile_y = row * TILE_PX;

        match tiles_bytes {
            Ok(bytes) => {
                let mut tile = image::load_from_memory(bytes).ok().unwrap_or_default();
                if tile.width() > TILE_PX {
                    tile = tile.thumbnail(TILE_PX, TILE_PX);
                }
                image::imageops::overlay(&mut collage, &tile, tile_x.into(), tile_y.into());
            }
            Err(_) => {
                continue;
            }
        };

        // Draw text

        if text {
            let text_color = Rgba([255, 255, 255, 255]);
            let outline_color = Rgba([0, 0, 0, 255]);
            let mut text_image = RgbaImage::from_pixel(TILE_PX, TILE_PX, Rgba([0, 0, 0, 0]));

            let mut draw_text = |x: i32, y: i32, text: &str, fg: bool| {
                draw_text_mut(
                    &mut text_image,
                    if fg { text_color } else { outline_color },
                    x,
                    y,
                    Scale::uniform(FONT_SIZE),
                    &font,
                    text,
                )
            };

            let mut draw_text_with_outline = |x: i32, y: i32, text: &str| {
                draw_text(x - 2, y - 2, text, false);
                draw_text(x - 2, y, text, false);
                draw_text(x - 2, y + 2, text, false);
                draw_text(x, y - 2, text, false);
                draw_text(x, y + 2, text, false);
                draw_text(x + 2, y - 2, text, false);
                draw_text(x + 2, y, text, false);
                draw_text(x + 2, y + 2, text, false);

                draw_text(x, y, text, true);
            };

            let tile_size = TILE_PX as i32;

            draw_text_with_outline(10, tile_size - 70, &album.name);
            draw_text_with_outline(10, tile_size - 50, &album.artist);
            draw_text_with_outline(
                10,
                tile_size - 30,
                &format!("{} plays", album.user_playcount),
            );

            image::imageops::overlay(&mut collage, &text_image, tile_x.into(), tile_y.into());
        }
    }

    // save collage to a file collage.jpg
    // collage.save("collage.jpg").unwrap();

    let mut jpeg_bytes: Vec<u8> = Vec::new();
    let mut encoder = JpegEncoder::new(&mut jpeg_bytes);
    encoder
        .encode_image(&collage)
        .expect("Failed to encode JPEG");

    Ok(jpeg_bytes)
}
