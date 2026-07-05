//! Embedding provider trait for the local memory provider.
//!
//! The local provider must not call network APIs. If no local embedding
//! provider is configured, records store `embedding_state = "missing"` and
//! semantic score is absent from ranking.

/// Trait for producing embedding vectors from text.
///
/// Implementations may use local models (e.g., ONNX, llama.cpp).
/// Test-only deterministic embeddings must live under test modules.
pub trait MemoryEmbeddingProvider: Send + Sync + 'static {
    /// Produce an embedding vector for the given text.
    ///
    /// Returns `None` if embedding is unavailable or fails.
    /// The returned vector's length is the embedding dimension.
    fn embed(&self, text: &str) -> Option<Vec<f32>>;

    /// The dimension of vectors produced by this provider.
    fn dimension(&self) -> usize;

    /// A short identifier for the embedding model (e.g., "all-MiniLM-L6-v2").
    fn model_id(&self) -> &str;
}

/// Cosine similarity between two vectors.
///
/// Returns `None` if vectors have different lengths or are zero-length.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for i in 0..a.len() {
        let va = a[i] as f64;
        let vb = b[i] as f64;
        dot += va * vb;
        norm_a += va * va;
        norm_b += vb * vb;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return Some(0.0);
    }

    let similarity = dot / (norm_a.sqrt() * norm_b.sqrt());
    // Clamp to [0.0, 1.0] for ranking purposes
    Some((similarity as f32).clamp(0.0, 1.0))
}

/// Serialize a vector of f32s to little-endian bytes.
pub fn serialize_vector_le(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &f in v {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

/// Deserialize a vector of f32s from little-endian bytes.
pub fn deserialize_vector_le(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    let count = bytes.len() / 4;
    let mut vec = Vec::with_capacity(count);
    for i in 0..count {
        let start = i * 4;
        let arr: [u8; 4] = bytes[start..start + 4].try_into().ok()?;
        vec.push(f32::from_le_bytes(arr));
    }
    Some(vec)
}
