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

        let model_params = LlamaModelParams::default()
            .with_use_mmap(true)
            .with_use_mlock(false);
        tracing::info!(
            "Loading GGUF model via llama.cpp (mmap=true, mlock=false): {}",
            model_path.display()
        );

        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| anyhow::anyhow!("Failed to load GGUF model: {:?}", e))?;

        tracing::info!("Model loaded. Vocab size: {}", model.n_vocab());

        Ok(Self { model, backend })
    }

    pub fn generate(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        let n_ctx = 512;
        let ctx_params = LlamaContextParams::default().with_n_ctx(std::num::NonZeroU32::new(n_ctx));

        tracing::info!(
            "Qwen2Infer::generate start: prompt_chars={}, prompt_bytes={}, max_tokens={}, n_ctx={}",
            prompt.chars().count(),
            prompt.len(),
            max_tokens,
            n_ctx
        );

        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

        let tokens = self
            .model
            .str_to_token(prompt, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {:?}", e))?;

        tracing::info!("Tokenization completed: {} tokens", tokens.len());

        if tokens.is_empty() {
            return Err(anyhow::anyhow!("Prompt produced no tokens. Try sending a non-empty message."));
        }

        if tokens.len() > (n_ctx as usize).saturating_sub(max_tokens) {
            return Err(anyhow::anyhow!(
                "Prompt too long ({} tokens) for context size {} with {} max output tokens",
                tokens.len(),
                n_ctx,
                max_tokens
            ));
        }

        let mut batch = LlamaBatch::new(n_ctx as usize, 1);
        let last_idx = (tokens.len() - 1) as i32;
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i as i32 == last_idx)
                .map_err(|_| anyhow::anyhow!("Failed to add token to batch"))?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| anyhow::anyhow!("Decode failed: {:?}", e))?;

        tracing::info!("Initial decode completed: batch_tokens={}", batch.n_tokens());

        let mut sampler =
            LlamaSampler::chain_simple([LlamaSampler::temp(0.7), LlamaSampler::dist(299792458)]);

        let mut output_tokens: Vec<LlamaToken> = Vec::new();
        let mut n_cur = batch.n_tokens();

        if n_cur <= 0 {
            return Err(anyhow::anyhow!("Model context is empty after prompt decoding."));
        }

        for _ in 0..max_tokens {
            // `sample()` expects an output-row index from the last decode, not the absolute
            // token position in the context. In our batching pattern only the last decoded
            // token has logits enabled, so `-1` is the correct "last output" selector.
            let token = sampler.sample(&ctx, -1);

            tracing::debug!("Sampled token: {}", token);

            if self.model.is_eog_token(token) {
                tracing::info!("Encountered EOG token, stopping generation");
                break;
            }

            output_tokens.push(token);

            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
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

        tracing::info!("Generation finished: output_chars={}, output_bytes={}", output.chars().count(), output.len());

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::Qwen2Infer;
    use std::path::PathBuf;

    #[test]
    #[ignore = "Loads a local GGUF file and runs real inference."]
    fn ignored_short_prompt_smoke_test() {
        let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../hydra_mobile/assets/models/qwen2.5-0.5b.gguf");
        assert!(
            model_path.exists(),
            "expected bundled smoke-test model at {}",
            model_path.display()
        );

        let mut infer = Qwen2Infer::load(&model_path, None).expect("model should load");
        infer
            .generate("hi", 8)
            .expect("short prompt generation should not fail");
    }
}
