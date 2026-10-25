//! Minimal GGUF header reader.
//!
//! Only the key/value metadata at the start of the file is read (never the weights), so this
//! works on a partial download or the first few megabytes fetched with an HTTP range request.
//! Files are untrusted: every length is bounded before anything is allocated.

use std::collections::BTreeMap;
use std::io::{self, Read};

use crate::gate::ModelShape;

const MAGIC: &[u8; 4] = b"GGUF";
const MAX_STRING: u64 = 1 << 20;
const MAX_KV: u64 = 100_000;
const MAX_ARRAY: u64 = 10_000_000;

#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("not a GGUF file")]
    BadMagic,
    #[error("unsupported GGUF version {0}")]
    Version(u32),
    #[error("corrupt GGUF header: {0}")]
    Corrupt(String),
    #[error("GGUF header is missing `{0}`")]
    Missing(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Metadata values we keep. Arrays are reduced to their numeric maximum (enough for
/// per-layer head counts) and strings arrays are skipped.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// Largest number in a numeric array, or `None` for empty / non-numeric arrays.
    ArrayMax(Option<f64>),
}

impl Value {
    fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(i) => u64::try_from(*i).ok(),
            Value::ArrayMax(Some(f)) if *f >= 0.0 => Some(*f as u64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Header {
    pub version: u32,
    pub tensor_count: u64,
    pub metadata: BTreeMap<String, Value>,
}

impl Header {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn architecture(&self) -> Option<&str> {
        match self.get("general.architecture") {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self.get("general.name") {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }

    fn arch_u64(&self, suffix: &str) -> Option<u64> {
        let arch = self.architecture()?;
        self.get(&format!("{arch}.{suffix}"))
            .and_then(Value::as_u64)
    }

    /// The numbers the hardware gate needs.
    pub fn shape(&self) -> Result<ModelShape, GgufError> {
        let need = |suffix: &str| {
            self.arch_u64(suffix)
                .ok_or_else(|| GgufError::Missing(suffix.to_string()))
        };
        let layers = need("block_count")?;
        let heads = need("attention.head_count")?.max(1);
        let kv_heads = self.arch_u64("attention.head_count_kv").unwrap_or(heads);
        let head_dim = match self.arch_u64("attention.key_length") {
            Some(k) => k,
            None => need("embedding_length")? / heads,
        };
        let context_length = self.arch_u64("context_length").unwrap_or(4096);
        let experts = self.arch_u64("expert_count").unwrap_or(0);
        let experts_used = self.arch_u64("expert_used_count").unwrap_or(0);
        let to_u32 =
            |v: u64| u32::try_from(v).map_err(|_| GgufError::Corrupt("value too large".into()));
        Ok(ModelShape {
            layers: to_u32(layers)?,
            kv_heads: to_u32(kv_heads)?,
            head_dim: to_u32(head_dim)?,
            context_length: to_u32(context_length)?,
            active_fraction: if experts > 0 && experts_used > 0 && experts_used < experts {
                // Rough: shared weights are not split out, so this overestimates speed a bit.
                Some(experts_used as f64 / experts as f64)
            } else {
                None
            },
        })
    }
}

/// Read the header from the start of a GGUF stream.
pub fn read_header(r: &mut impl Read) -> Result<Header, GgufError> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(GgufError::BadMagic);
    }
    let version = read_u32(r)?;
    if !(2..=3).contains(&version) {
        return Err(GgufError::Version(version));
    }
    let tensor_count = read_u64(r)?;
    let kv_count = read_u64(r)?;
    if kv_count > MAX_KV {
        return Err(GgufError::Corrupt(format!("{kv_count} metadata entries")));
    }
    let mut metadata = BTreeMap::new();
    for _ in 0..kv_count {
        let key = read_string(r)?;
        let ty = read_u32(r)?;
        let value = read_value(r, ty)?;
        if let Some(v) = value {
            metadata.insert(key, v);
        }
    }
    Ok(Header {
        version,
        tensor_count,
        metadata,
    })
}

/// Read the header of a local `.gguf` file.
pub fn read_file(path: &std::path::Path) -> Result<Header, GgufError> {
    let file = std::fs::File::open(path)?;
    read_header(&mut io::BufReader::with_capacity(1 << 16, file))
}

fn read_value(r: &mut impl Read, ty: u32) -> Result<Option<Value>, GgufError> {
    Ok(Some(match ty {
        0 => Value::Int(read_n::<1>(r)?[0] as i64),
        1 => Value::Int(read_n::<1>(r)?[0] as i8 as i64),
        2 => Value::Int(u16::from_le_bytes(read_n(r)?) as i64),
        3 => Value::Int(i16::from_le_bytes(read_n(r)?) as i64),
        4 => Value::Int(read_u32(r)? as i64),
        5 => Value::Int(i32::from_le_bytes(read_n(r)?) as i64),
        6 => Value::Float(f32::from_le_bytes(read_n(r)?) as f64),
        7 => Value::Bool(read_n::<1>(r)?[0] != 0),
        8 => Value::Str(read_string(r)?),
        9 => {
            let item_ty = read_u32(r)?;
            let len = read_u64(r)?;
            if len > MAX_ARRAY {
                return Err(GgufError::Corrupt(format!("array of {len} items")));
            }
            let mut max: Option<f64> = None;
            for _ in 0..len {
                let item = read_value(r, item_ty)?;
                let n = match item {
                    Some(Value::Int(i)) => Some(i as f64),
                    Some(Value::Float(f)) => Some(f),
                    _ => None,
                };
                if let Some(n) = n {
                    max = Some(max.map_or(n, |m| m.max(n)));
                }
            }
            Value::ArrayMax(max)
        }
        10 => Value::Int(i64::try_from(read_u64(r)?).unwrap_or(i64::MAX)),
        11 => Value::Int(i64::from_le_bytes(read_n(r)?)),
        12 => Value::Float(f64::from_le_bytes(read_n(r)?)),
        other => return Err(GgufError::Corrupt(format!("unknown value type {other}"))),
    }))
}

fn read_n<const N: usize>(r: &mut impl Read) -> io::Result<[u8; N]> {
    let mut b = [0u8; N];
    r.read_exact(&mut b)?;
    Ok(b)
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    Ok(u32::from_le_bytes(read_n(r)?))
}

fn read_u64(r: &mut impl Read) -> io::Result<u64> {
    Ok(u64::from_le_bytes(read_n(r)?))
}

fn read_string(r: &mut impl Read) -> Result<String, GgufError> {
    let len = read_u64(r)?;
    if len > MAX_STRING {
        return Err(GgufError::Corrupt(format!("string of {len} bytes")));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a small synthetic GGUF header (used by other tests too).
    pub(crate) fn sample_header(arch: &str, kv: &[(&str, u32)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&((kv.len() + 2) as u64).to_le_bytes());
        let put_str = |b: &mut Vec<u8>, s: &str| {
            b.extend_from_slice(&(s.len() as u64).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        };
        put_str(&mut b, "general.architecture");
        b.extend_from_slice(&8u32.to_le_bytes());
        put_str(&mut b, arch);
        // A string array (like a tokenizer vocabulary) that must be skipped.
        put_str(&mut b, "tokenizer.ggml.tokens");
        b.extend_from_slice(&9u32.to_le_bytes());
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend_from_slice(&2u64.to_le_bytes());
        put_str(&mut b, "<s>");
        put_str(&mut b, "</s>");
        for (k, v) in kv {
            put_str(&mut b, &format!("{arch}.{k}"));
            b.extend_from_slice(&4u32.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    #[test]
    fn reads_shape_from_header() {
        let bytes = sample_header(
            "llama",
            &[
                ("block_count", 16),
                ("context_length", 131072),
                ("embedding_length", 2048),
                ("attention.head_count", 32),
                ("attention.head_count_kv", 8),
            ],
        );
        let h = read_header(&mut bytes.as_slice()).unwrap();
        assert_eq!(h.architecture(), Some("llama"));
        let s = h.shape().unwrap();
        assert_eq!((s.layers, s.kv_heads, s.head_dim), (16, 8, 64));
        assert_eq!(s.context_length, 131072);
        assert_eq!(s.active_fraction, None);
    }

    #[test]
    fn rejects_garbage_and_huge_lengths() {
        assert!(matches!(
            read_header(&mut &b"NOPE\x03\0\0\0"[..]),
            Err(GgufError::BadMagic)
        ));
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&1u64.to_le_bytes());
        b.extend_from_slice(&u64::MAX.to_le_bytes()); // key length
        assert!(matches!(
            read_header(&mut b.as_slice()),
            Err(GgufError::Corrupt(_))
        ));
    }

    #[test]
    fn missing_fields_are_reported() {
        let bytes = sample_header("llama", &[("context_length", 4096)]);
        let h = read_header(&mut bytes.as_slice()).unwrap();
        assert!(matches!(h.shape(), Err(GgufError::Missing(_))));
    }
}
