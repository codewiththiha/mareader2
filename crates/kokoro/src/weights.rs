//! Reading PyTorch files: the model's weights, and a voice's style rows.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use candle_core::pickle::{Object, Stack};
use candle_core::{DType, Device, Tensor};

use crate::Error;

/// The model file's top-level parts.
const PARTS: [&str; 5] = [
    "bert",
    "bert_encoder",
    "decoder",
    "predictor",
    "text_encoder",
];

/// Every weight by name, `module.` wrappers dropped, as `f32`.
pub fn read_model(path: &Path) -> Result<HashMap<String, Tensor>, Error> {
    let mut all = HashMap::new();
    if path.extension().is_some_and(|e| e == "safetensors") {
        for (name, t) in candle_core::safetensors::load(path, &Device::Cpu)? {
            all.insert(name, t.to_dtype(DType::F32)?);
        }
        return Ok(all);
    }
    for part in PARTS {
        let tensors = candle_core::pickle::read_all_with_key(path, Some(part))
            .map_err(|e| Error::Load(format!("{} ({part}): {e}", path.display())))?;
        for (name, t) in tensors {
            let name = format!("{part}.{name}").replace(".module.", ".");
            all.insert(name, t.to_dtype(DType::F32)?);
        }
    }
    Ok(all)
}

fn bad(path: &Path, what: impl std::fmt::Display) -> Error {
    Error::Load(format!("{}: {what}", path.display()))
}

/// A `.pt` file holding one bare tensor, as a voice pack does.
pub(crate) fn read_tensor(path: &Path) -> Result<Tensor, Error> {
    let file = File::open(path).map_err(|e| bad(path, e))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| bad(path, e))?;
    let pickle = zip
        .file_names()
        .filter_map(Result::ok)
        .find(|n| n.ends_with("data.pkl"))
        .map(std::borrow::Cow::into_owned)
        .ok_or_else(|| bad(path, "no data.pkl inside"))?;
    let mut stack = Stack::empty();
    {
        let entry = zip.by_name(&pickle).map_err(|e| bad(path, e))?;
        stack.read_loop(&mut BufReader::new(entry))?;
    }
    let dir = Path::new(pickle.strip_suffix(".pkl").unwrap_or(&pickle));
    let info = stack
        .finalize()?
        .into_tensor_info(Object::Unicode(String::new()), dir)?
        .ok_or_else(|| bad(path, "not a single tensor"))?;
    if !info.layout.is_contiguous() {
        return Err(bad(path, "a non-contiguous tensor"));
    }
    let shape = info.layout.shape().clone();
    let size = shape.elem_count() * info.dtype.size_in_bytes();
    let skip = info.layout.start_offset() * info.dtype.size_in_bytes();
    let entry = zip.by_name(&info.path).map_err(|e| bad(path, e))?;
    let mut data = Vec::with_capacity(size);
    let mut rest = entry.take((skip + size) as u64);
    std::io::copy(&mut rest.by_ref().take(skip as u64), &mut std::io::sink())
        .map_err(|e| bad(path, e))?;
    rest.read_to_end(&mut data).map_err(|e| bad(path, e))?;
    if data.len() != size {
        return Err(bad(path, "the tensor's data is cut short"));
    }
    let t = Tensor::from_raw_buffer(&data, info.dtype, shape.dims(), &Device::Cpu)?;
    Ok(t.to_dtype(DType::F32)?)
}
