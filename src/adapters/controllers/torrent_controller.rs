use log::info;
use sha1::{Digest, Sha1};
use std::net::SocketAddr;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Write};

use crate::domain::entities::{Peer, Torrent, TorrentInfo};
use crate::domain::errors::TorrentError;
use crate::adapters::presenters::torrent_presenter::print_torrent_info;
use crate::usecases::{read_file::read_file,
                      peer_discovery::request_peers,
                      peer_handshake::download_piece,
                      extract_torrent_info::extract_torrent_info};

pub async fn handle_torrent(file_path: &str) -> Result<()> {
    info!("Reading torrent file: {}\n", file_path);

    let buffer = read_file(file_path).await?;
    let decoded_value = decode_torrent(&buffer)?;
    let info_hash_hex = calculate_info_hash(&decoded_value)?;
    let torrent_info = extract_torrent_info(&decoded_value, &info_hash_hex);

    print_torrent_info(&torrent_info);
    println!("\n");

    let peers = request_peers(&torrent_info).await?;
    download_pieces_from_peers(peers, &info_hash_hex, &torrent_info).await;

    info!("Torrent handling completed successfully.\n");
    Ok(())
}

fn decode_torrent(buffer: &[u8]) -> Result<Torrent, TorrentError> {
    serde_bencode::from_bytes(buffer)
        .map_err(|e| TorrentError::TorrentParsingError(e.to_string()))
}

fn calculate_info_hash(decoded_value: &Torrent) -> Result<String> {
    let mut hasher = Sha1::new();
    let info_encoded = serde_bencode::to_bytes(&decoded_value.info)
        .context("Failed to re-encode info section")?;

    hasher.update(&info_encoded);
    Ok(hex::encode(hasher.finalize()))
}

async fn download_pieces_from_peers(peers: Vec<Peer>, info_hash_hex: &str, torrent_info: &TorrentInfo) {
    let piece_length = torrent_info.piece_length.unwrap() as usize;
    let num_pieces = torrent_info.pieces.as_ref().unwrap().len();

    for piece_index in 0..num_pieces {
        let mut piece_downloaded = false;

        for peer in &peers {
            let peer_addr: SocketAddr = format!("{}:{}", peer.ip, peer.port)
                .parse()
                .context("Invalid peer address format")
                .unwrap();

            match download_piece(&peer_addr, &hex::decode(info_hash_hex).unwrap(), piece_index as u32, piece_length, torrent_info).await {
                Ok(_) => {
                    info!("Downloaded piece {} successfully from peer: {}\n", piece_index, peer_addr);
                    piece_downloaded = true;
                    break;
                },
                Err(e) => {
                    eprintln!("Failed to download piece {} from peer {}: {:?}", piece_index, peer_addr, e);
                    if e.to_string().contains("Failed to read length prefix") || e.to_string().contains("Invalid piece message") {
                        break;
                    }
                }
            }
        }

        if !piece_downloaded {
            info!("Failed to download piece {} from any peer\n", piece_index);
        }
    }

    if combine_pieces_into_file(num_pieces, piece_length).is_ok() {
        info!("Successfully combined all pieces into the final file.\n");
    } else {
        info!("Failed to combine pieces into the final file.\n");
    }
}

fn combine_pieces_into_file(num_pieces: usize, piece_length: usize) -> Result<()> {
    let final_path = "./src/final_file";
    let mut final_file = File::create(final_path).context("Failed to create final file")?;

    for piece_index in 0..num_pieces {
        let path = format!("/tmp/test-piece-{}.tmp", piece_index);
        let mut piece_file = File::open(&path).context("Failed to open piece file")?;
        let mut buffer = Vec::with_capacity(piece_length);
        piece_file.read_to_end(&mut buffer).context("Failed to read piece file")?;
        final_file.write_all(&buffer).context("Failed to write piece to final file")?;
    }

    println!("Final file created at {}", final_path);
    Ok(())
}
