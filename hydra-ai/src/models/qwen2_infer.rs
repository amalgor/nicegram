use anyhow::Result;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::path::PathBuf;

pub struct Qwen2Infer {
    model: LlamaModel,
    backend: LlamaBackend,
}

impl Qwen2Infer {
    pub fn load(model_path: &PathBuf, _tokenizer_path: Option<&PathBuf>) -> Result<Self> {
        let backend = LlamaBackend::init()?;
        llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default());

        let model_params = LlamaModelParams::default();
        tracing::info!("Loading GGUF model via llama.cpp: {}", model_path.display());

        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| anyhow::anyhow!("Failed to load GGUF model: {:?}", e))?;

        tracing::info!("Model loaded. Vocab size: {}", model.n_vocab());

        Ok(Self { model, backend })
    }

    pub fn generate(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(2048));

        let mut ctx = self.model.new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

        let tokens = self.model.str_to_token(prompt, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {:?}", e))?;

        let mut batch = LlamaBatch::new(2048, 1);
        let last_idx = (tokens.len() - 1) as i32;
        for (i, token) in tokens.iter().enumerate() {
            batch.add(*token, i as i32, &[0], i as i32 == last_idx)
                .map_err(|_| anyhow::anyhow!("Failed to add token to batch"))?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| anyhow::anyhow!("Decode failed: {:?}", e))?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(299792458),
        ]);

        let mut output_tokens: Vec<LlamaToken> = Vec::new();
        let mut n_cur = batch.n_tokens();

        for _ in 0..max_tokens {
            let token = sampler.sample(&ctx, n_cur - 1);
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                break;
            }

            output_tokens.push(token);

            batch.clear();
            batch.add(token, n_cur, &[0], true)
                .map_err(|_| anyhow::anyhow!("Failed to add token to batch"))?;

            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("Decode failed: {:?}", e))?;

            n_cur += 1;
        }

        let mut output_bytes: Vec<u8> = Vec::new();
        for t in &output_tokens {
            match self.model.token_to_piece_bytes(*t, 64, true, None) {
                Ok(bytes) => output_bytes.extend_from_slice(&bytes),
                Err(_) => {}
            }
        }
        let output = String::from_utf8_lossy(&output_bytes).into_owned();

        Ok(output)
    }
}
