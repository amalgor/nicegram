use anyhow::Result;
use hydra_p2p::P2PHandle;
use libp2p::PeerId;

pub struct OnionRouter {
    p2p: P2PHandle,
}

impl OnionRouter {
    pub fn new(p2p: P2PHandle) -> Self {
        Self { p2p }
    }

    /// Build a multi-hop path and return a Stream that connects to the final target via intermediaries.
    pub async fn build_circuit(&self, path: &[PeerId], target: &str) -> Result<libp2p::Stream> {
        if path.is_empty() {
            return Err(anyhow::anyhow!("Empty path for onion routing"));
        }

        let mut current_stream = self
            .p2p
            .open_stream(path[0], hydra_p2p::TUNNEL_PROTOCOL)
            .await?;

        // Inform each hop about the next hop
        for i in 1..path.len() {
            use libp2p::futures::{AsyncReadExt as _, AsyncWriteExt as _};

            let next_hop = path[i];
            let target_str = format!("peer:{}", next_hop);
            let target_bytes = target_str.as_bytes();
            let len_bytes = (target_bytes.len() as u16).to_be_bytes();

            current_stream.write_all(&len_bytes).await?;
            current_stream.write_all(target_bytes).await?;

            let mut resp = [0u8; 1];
            current_stream.read_exact(&mut resp).await?;

            if resp[0] != 0x00 {
                return Err(anyhow::anyhow!(
                    "Hop {} rejected connection to {}",
                    path[i - 1],
                    next_hop
                ));
            }
        }

        // Inform the final hop about the ultimate target
        use libp2p::futures::{AsyncReadExt as _, AsyncWriteExt as _};
        let target_bytes = target.as_bytes();
        let len_bytes = (target_bytes.len() as u16).to_be_bytes();

        current_stream.write_all(&len_bytes).await?;
        current_stream.write_all(target_bytes).await?;

        let mut resp = [0u8; 1];
        current_stream.read_exact(&mut resp).await?;

        if resp[0] != 0x00 {
            return Err(anyhow::anyhow!(
                "Final hop rejected connection to {}",
                target
            ));
        }

        Ok(current_stream)
    }
}
