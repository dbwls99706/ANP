use async_trait::async_trait;
use serde_json::{json, Value};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// AES-256-CBC encryption/decryption node.
/// Pure Rust implementation — no external crypto dependencies.
/// Uses PKCS7 padding, 256-bit keys, 128-bit IV.
pub struct AesNode {
    operation: String,
    key_env: String,
}

impl AesNode {
    pub fn new() -> Self {
        Self {
            operation: "encrypt".to_string(),
            key_env: "AES_KEY".to_string(),
        }
    }
}

impl Default for AesNode {
    fn default() -> Self {
        Self::new()
    }
}

// AES S-box
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

// Inverse S-box
const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut p: u8 = 0;
    for _ in 0..8 {
        if b & 1 != 0 {
            p ^= a;
        }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    p
}

fn sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() {
        *b = SBOX[*b as usize];
    }
}

fn inv_sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() {
        *b = INV_SBOX[*b as usize];
    }
}

fn shift_rows(state: &mut [u8; 16]) {
    // Row 1: shift left 1
    let tmp = state[1];
    state[1] = state[5];
    state[5] = state[9];
    state[9] = state[13];
    state[13] = tmp;
    // Row 2: shift left 2
    let (t0, t1) = (state[2], state[6]);
    state[2] = state[10];
    state[6] = state[14];
    state[10] = t0;
    state[14] = t1;
    // Row 3: shift left 3 (= right 1)
    let tmp = state[15];
    state[15] = state[11];
    state[11] = state[7];
    state[7] = state[3];
    state[3] = tmp;
}

fn inv_shift_rows(state: &mut [u8; 16]) {
    // Row 1: shift right 1
    let tmp = state[13];
    state[13] = state[9];
    state[9] = state[5];
    state[5] = state[1];
    state[1] = tmp;
    // Row 2: shift right 2
    let (t0, t1) = (state[2], state[6]);
    state[2] = state[10];
    state[6] = state[14];
    state[10] = t0;
    state[14] = t1;
    // Row 3: shift right 3 (= left 1)
    let tmp = state[3];
    state[3] = state[7];
    state[7] = state[11];
    state[11] = state[15];
    state[15] = tmp;
}

fn mix_columns(state: &mut [u8; 16]) {
    for col in 0..4 {
        let i = col * 4;
        let (a0, a1, a2, a3) = (state[i], state[i + 1], state[i + 2], state[i + 3]);
        state[i] = gf_mul(a0, 2) ^ gf_mul(a1, 3) ^ a2 ^ a3;
        state[i + 1] = a0 ^ gf_mul(a1, 2) ^ gf_mul(a2, 3) ^ a3;
        state[i + 2] = a0 ^ a1 ^ gf_mul(a2, 2) ^ gf_mul(a3, 3);
        state[i + 3] = gf_mul(a0, 3) ^ a1 ^ a2 ^ gf_mul(a3, 2);
    }
}

fn inv_mix_columns(state: &mut [u8; 16]) {
    for col in 0..4 {
        let i = col * 4;
        let (a0, a1, a2, a3) = (state[i], state[i + 1], state[i + 2], state[i + 3]);
        state[i] = gf_mul(a0, 14) ^ gf_mul(a1, 11) ^ gf_mul(a2, 13) ^ gf_mul(a3, 9);
        state[i + 1] = gf_mul(a0, 9) ^ gf_mul(a1, 14) ^ gf_mul(a2, 11) ^ gf_mul(a3, 13);
        state[i + 2] = gf_mul(a0, 13) ^ gf_mul(a1, 9) ^ gf_mul(a2, 14) ^ gf_mul(a3, 11);
        state[i + 3] = gf_mul(a0, 11) ^ gf_mul(a1, 13) ^ gf_mul(a2, 9) ^ gf_mul(a3, 14);
    }
}

fn add_round_key(state: &mut [u8; 16], round_key: &[u8]) {
    for (i, b) in state.iter_mut().enumerate() {
        *b ^= round_key[i];
    }
}

fn key_expansion(key: &[u8; 32]) -> Vec<u8> {
    let nk = 8; // AES-256
    let nr = 14;
    let mut w = vec![0u8; 16 * (nr + 1)];
    w[..32].copy_from_slice(key);

    for i in nk..(4 * (nr + 1)) {
        let mut temp = [0u8; 4];
        temp.copy_from_slice(&w[(i - 1) * 4..i * 4]);

        if i % nk == 0 {
            // RotWord
            let t = temp[0];
            temp[0] = temp[1];
            temp[1] = temp[2];
            temp[2] = temp[3];
            temp[3] = t;
            // SubWord
            for b in temp.iter_mut() {
                *b = SBOX[*b as usize];
            }
            temp[0] ^= RCON[(i / nk) - 1];
        } else if i % nk == 4 {
            for b in temp.iter_mut() {
                *b = SBOX[*b as usize];
            }
        }

        for j in 0..4 {
            w[i * 4 + j] = w[(i - nk) * 4 + j] ^ temp[j];
        }
    }
    w
}

fn aes256_encrypt_block(block: &[u8; 16], expanded_key: &[u8]) -> [u8; 16] {
    let nr = 14;
    let mut state = *block;

    // Column-major ordering for AES state
    let mut col_state = [0u8; 16];
    for row in 0..4 {
        for col in 0..4 {
            col_state[col * 4 + row] = state[row * 4 + col];
        }
    }
    state = col_state;

    add_round_key(&mut state, &expanded_key[0..16]);

    for round in 1..nr {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        add_round_key(&mut state, &expanded_key[round * 16..(round + 1) * 16]);
    }

    sub_bytes(&mut state);
    shift_rows(&mut state);
    add_round_key(&mut state, &expanded_key[nr * 16..(nr + 1) * 16]);

    // Back to row-major
    let mut row_state = [0u8; 16];
    for row in 0..4 {
        for col in 0..4 {
            row_state[row * 4 + col] = state[col * 4 + row];
        }
    }
    row_state
}

fn aes256_decrypt_block(block: &[u8; 16], expanded_key: &[u8]) -> [u8; 16] {
    let nr = 14;
    let mut state = *block;

    // Column-major
    let mut col_state = [0u8; 16];
    for row in 0..4 {
        for col in 0..4 {
            col_state[col * 4 + row] = state[row * 4 + col];
        }
    }
    state = col_state;

    add_round_key(&mut state, &expanded_key[nr * 16..(nr + 1) * 16]);

    for round in (1..nr).rev() {
        inv_shift_rows(&mut state);
        inv_sub_bytes(&mut state);
        add_round_key(&mut state, &expanded_key[round * 16..(round + 1) * 16]);
        inv_mix_columns(&mut state);
    }

    inv_shift_rows(&mut state);
    inv_sub_bytes(&mut state);
    add_round_key(&mut state, &expanded_key[0..16]);

    // Back to row-major
    let mut row_state = [0u8; 16];
    for row in 0..4 {
        for col in 0..4 {
            row_state[row * 4 + col] = state[col * 4 + row];
        }
    }
    row_state
}

fn pkcs7_pad(data: &[u8]) -> Vec<u8> {
    let block_size = 16;
    let padding = block_size - (data.len() % block_size);
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat_n(padding as u8, padding));
    padded
}

fn pkcs7_unpad(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err("invalid padded data");
    }
    let padding = data[data.len() - 1] as usize;
    if padding == 0 || padding > 16 {
        return Err("invalid padding value");
    }
    if data.len() < padding {
        return Err("padding exceeds data length");
    }
    for &b in &data[data.len() - padding..] {
        if b as usize != padding {
            return Err("invalid padding bytes");
        }
    }
    Ok(data[..data.len() - padding].to_vec())
}

fn derive_key(passphrase: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    let bytes = passphrase.as_bytes();
    // PBKDF2-like derivation using FNV iterations
    let mut state: u64 = 14695981039346656037;
    for round in 0..4096u32 {
        for &b in bytes {
            state ^= b as u64;
            state = state.wrapping_mul(1099511628211);
        }
        state ^= round as u64;
        state = state.wrapping_mul(1099511628211);
    }
    for (i, key_byte) in key.iter_mut().enumerate() {
        let mut h = state;
        h ^= i as u64;
        h = h.wrapping_mul(1099511628211);
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        *key_byte = (h >> 24) as u8;
    }
    key
}

fn generate_iv() -> [u8; 16] {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mut iv = [0u8; 16];
    let nanos = now.as_nanos();
    for (i, iv_byte) in iv.iter_mut().enumerate() {
        *iv_byte = ((nanos >> (i * 8)) & 0xFF) as u8;
    }
    // Mix with a simple PRNG
    let mut state = nanos as u64;
    for b in iv.iter_mut() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *b ^= (state >> 33) as u8;
    }
    iv
}

fn to_hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn from_hex(hex: &str) -> Result<Vec<u8>, &'static str> {
    if !hex.len().is_multiple_of(2) {
        return Err("odd hex length");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| "invalid hex"))
        .collect()
}

fn aes256_cbc_encrypt(plaintext: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    let expanded = key_expansion(key);
    let padded = pkcs7_pad(plaintext);
    let mut ciphertext = Vec::with_capacity(padded.len());
    let mut prev_block = *iv;

    for chunk in padded.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        // XOR with previous ciphertext block (CBC)
        for i in 0..16 {
            block[i] ^= prev_block[i];
        }
        let encrypted = aes256_encrypt_block(&block, &expanded);
        prev_block = encrypted;
        ciphertext.extend_from_slice(&encrypted);
    }
    ciphertext
}

fn aes256_cbc_decrypt(
    ciphertext: &[u8],
    key: &[u8; 32],
    iv: &[u8; 16],
) -> Result<Vec<u8>, &'static str> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return Err("invalid ciphertext length");
    }
    let expanded = key_expansion(key);
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    let mut prev_block = *iv;

    for chunk in ciphertext.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let decrypted = aes256_decrypt_block(&block, &expanded);
        let mut plain_block = [0u8; 16];
        for i in 0..16 {
            plain_block[i] = decrypted[i] ^ prev_block[i];
        }
        prev_block.copy_from_slice(chunk);
        plaintext.extend_from_slice(&plain_block);
    }
    pkcs7_unpad(&plaintext)
}

#[async_trait]
impl NodeExecutor for AesNode {
    fn node_type(&self) -> &str {
        "aes"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "encrypt" | "decrypt" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}', expected 'encrypt' or 'decrypt'",
                        other
                    )));
                }
            }
        }
        if let Some(env) = config.get("key_env").and_then(|v| v.as_str()) {
            self.key_env = env.to_string();
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        let key_env = input
            .get("key_env")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.key_env);

        let passphrase = std::env::var(key_env).map_err(|_| {
            NodeError::ExecutionFailed(format!("environment variable '{}' not set", key_env))
        })?;
        let key = derive_key(&passphrase);

        match op {
            "encrypt" => {
                let data_str = input
                    .get("data")
                    .or_else(|| input.get("text"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("data or text field required".into()))?;

                let iv = generate_iv();
                let ciphertext = aes256_cbc_encrypt(data_str.as_bytes(), &key, &iv);

                Ok(NodeOutput::data(json!({
                    "ciphertext": to_hex(&ciphertext),
                    "iv": to_hex(&iv),
                    "algorithm": "AES-256-CBC",
                    "encoding": "hex",
                })))
            }
            "decrypt" => {
                let ciphertext_hex = input
                    .get("ciphertext")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("ciphertext field required".into()))?;
                let iv_hex = input
                    .get("iv")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("iv field required".into()))?;

                let ciphertext = from_hex(ciphertext_hex).map_err(|e| {
                    NodeError::ExecutionFailed(format!("invalid ciphertext hex: {}", e))
                })?;
                let iv_bytes = from_hex(iv_hex)
                    .map_err(|e| NodeError::ExecutionFailed(format!("invalid iv hex: {}", e)))?;
                if iv_bytes.len() != 16 {
                    return Err(NodeError::ExecutionFailed("IV must be 16 bytes".into()));
                }
                let mut iv = [0u8; 16];
                iv.copy_from_slice(&iv_bytes);

                let plaintext = aes256_cbc_decrypt(&ciphertext, &key, &iv)
                    .map_err(|e| NodeError::ExecutionFailed(format!("decryption failed: {}", e)))?;

                let text = String::from_utf8(plaintext).map_err(|e| {
                    NodeError::ExecutionFailed(format!("decrypted data is not valid UTF-8: {}", e))
                })?;

                Ok(NodeOutput::data(json!({
                    "plaintext": text,
                    "algorithm": "AES-256-CBC",
                })))
            }
            other => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[test]
    fn test_aes_block_roundtrip() {
        let key = [0x00u8; 32];
        let expanded = key_expansion(&key);
        let plaintext = [0x00u8; 16];
        let encrypted = aes256_encrypt_block(&plaintext, &expanded);
        let decrypted = aes256_decrypt_block(&encrypted, &expanded);
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_aes_block_nonzero() {
        let key = [0x01u8; 32];
        let expanded = key_expansion(&key);
        let plaintext = *b"Hello AES-256!! ";
        let encrypted = aes256_encrypt_block(&plaintext, &expanded);
        assert_ne!(plaintext.to_vec(), encrypted.to_vec());
        let decrypted = aes256_decrypt_block(&encrypted, &expanded);
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_pkcs7_pad_unpad() {
        let data = b"Hello World";
        let padded = pkcs7_pad(data);
        assert_eq!(padded.len(), 16);
        let unpadded = pkcs7_unpad(&padded).unwrap();
        assert_eq!(unpadded, data);
    }

    #[test]
    fn test_cbc_roundtrip() {
        let key = derive_key("test-passphrase");
        let iv = [0x42u8; 16];
        let plaintext = b"Hello World! AES-256-CBC works great.";
        let ciphertext = aes256_cbc_encrypt(plaintext, &key, &iv);
        let decrypted = aes256_cbc_decrypt(&ciphertext, &key, &iv).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_hex_roundtrip() {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let hex = to_hex(&data);
        assert_eq!(hex, "deadbeef");
        let back = from_hex(&hex).unwrap();
        assert_eq!(back, data);
    }

    #[tokio::test]
    async fn test_encrypt_decrypt() {
        std::env::set_var("AES_KEY", "my-secret-key-123");
        let mut node = AesNode::new();
        node.init(&json!({"operation": "encrypt"})).await.unwrap();

        let enc_output = node
            .execute(json!({"data": "Hello, World!"}), &ctx())
            .await
            .unwrap();
        let enc = &enc_output.ports["default"];
        let ciphertext = enc["ciphertext"].as_str().unwrap();
        let iv = enc["iv"].as_str().unwrap();
        assert_eq!(enc["algorithm"], "AES-256-CBC");

        let mut dec_node = AesNode::new();
        dec_node
            .init(&json!({"operation": "decrypt"}))
            .await
            .unwrap();
        let dec_output = dec_node
            .execute(json!({"ciphertext": ciphertext, "iv": iv}), &ctx())
            .await
            .unwrap();
        assert_eq!(dec_output.ports["default"]["plaintext"], "Hello, World!");
    }

    #[tokio::test]
    async fn test_invalid_key_env() {
        std::env::remove_var("MISSING_KEY_ENV_12345");
        let mut node = AesNode::new();
        node.init(&json!({"operation": "encrypt", "key_env": "MISSING_KEY_ENV_12345"}))
            .await
            .unwrap();
        let result = node.execute(json!({"data": "test"}), &ctx()).await;
        assert!(result.is_err());
    }
}
