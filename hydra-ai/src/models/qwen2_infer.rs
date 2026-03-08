use anyhow::Result;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_qwen2::ModelWeights;
use std::path::PathBuf;
use tokenizers::Tokenizer;

pub struct Qwen2Infer {
    model: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
}

impl Qwen2Infer {
    pub fn load(model_path: &PathBuf, tokenizer_path: Option<&PathBuf>) -> Result<Self> {
        let device = Device::Cpu;
        let mut file = std::fs::File::open(model_path)?;
        let gguf = candle_core::quantized::gguf_file::Content::read(&mut file)?;
        let model = ModelWeights::from_gguf(gguf, &mut file, &device)?;

        let tokenizer = if let Some(tp) = tokenizer_path {
            Tokenizer::from_file(tp)
                .map_err(|e| anyhow::anyhow!("Error loading tokenizer: {}", e))?
        } else {
            anyhow::bail!("Tokenizer path is required");
        };

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    pub fn generate(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        let tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!(e))?;
        let mut tokens = tokens.get_ids().to_vec();

        let mut logits_processor = LogitsProcessor::new(299792458, None, None);
        let mut generated_tokens = vec![];
        let mut index_pos = 0;

        for _ in 0..max_tokens {
            let context_size = if index_pos == 0 { tokens.len() } else { 1 };
            let start_pos = tokens.len() - context_size;

            let input = Tensor::new(&tokens[start_pos..], &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, index_pos)?;
            let logits = logits.squeeze(0)?.squeeze(0)?; // Assuming seq_len=1 for generation

            let next_token = logits_processor.sample(&logits)?;
            tokens.push(next_token);
            generated_tokens.push(next_token);
            index_pos += context_size;

            // Check if EOS or specific stop tokens are generated
            // Qwen2 EOS is often 151645 or 151643
            if next_token == 151645 || next_token == 151643 {
                break;
            }
        }

        let output = self
            .tokenizer
            .decode(&generated_tokens, true)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(output)
    }
}
