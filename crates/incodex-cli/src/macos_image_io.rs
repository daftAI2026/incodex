/**
 * [INPUT]: 依赖 macOS ImageIO/CoreFoundation 动态 framework、受限图像字节与已解码 CGImage
 * [OUTPUT]: 对外提供有界 2600px 静态图像转换，以及与视频首帧共享的 JPEG 0.85 编码
 * [POS]: 系统壁纸的 macOS 图像编码边界；静态源使用 CreateWithData，视频适配器传入 CGImage，不接受路径或网络来源
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::ffi::{c_char, c_void, CStr, CString};

const MAX_DIMENSION: i32 = 2600;
const JPEG_QUALITY: f64 = 0.85;
const CF_NUMBER_SINT32_TYPE: isize = 3;
#[cfg(test)]
const CF_NUMBER_SINT64_TYPE: isize = 4;
const CF_NUMBER_FLOAT64_TYPE: isize = 13;
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

type CfRef = *const c_void;
type CfMutableRef = *mut c_void;
type ReleaseFn = unsafe extern "C" fn(CfRef);
type CreateDataFn = unsafe extern "C" fn(CfRef, *const u8, isize) -> CfRef;
type CreateMutableDataFn = unsafe extern "C" fn(CfRef, isize) -> CfMutableRef;
type DataGetLengthFn = unsafe extern "C" fn(CfRef) -> isize;
type DataGetBytePtrFn = unsafe extern "C" fn(CfRef) -> *const u8;
type CreateNumberFn = unsafe extern "C" fn(CfRef, isize, *const c_void) -> CfRef;
type CreateStringFn = unsafe extern "C" fn(CfRef, *const c_char, u32) -> CfRef;
type CreateDictionaryFn =
    unsafe extern "C" fn(CfRef, *const CfRef, *const CfRef, isize, CfRef, CfRef) -> CfRef;
type CreateSourceWithDataFn = unsafe extern "C" fn(CfRef, CfRef, CfRef) -> CfRef;
#[cfg(test)]
type CopyPropertiesFn = unsafe extern "C" fn(CfRef, isize, CfRef) -> CfRef;
type CreateThumbnailFn = unsafe extern "C" fn(CfRef, isize, CfRef) -> CfRef;
type CreateDestinationFn = unsafe extern "C" fn(CfMutableRef, CfRef, isize, CfRef) -> CfMutableRef;
type AddImageFn = unsafe extern "C" fn(CfMutableRef, CfRef, CfRef);
type FinalizeFn = unsafe extern "C" fn(CfMutableRef) -> bool;
#[cfg(test)]
type DictionaryGetValueFn = unsafe extern "C" fn(CfRef, CfRef) -> CfRef;
#[cfg(test)]
type NumberGetValueFn = unsafe extern "C" fn(CfRef, isize, *mut c_void) -> bool;

/// 从受限读取的图像字节读取原始像素尺寸，不执行路径访问或网络加载。
#[cfg(test)]
pub(crate) fn image_dimensions(input: &[u8]) -> Result<(u32, u32), String> {
    if input.is_empty() || input.len() > isize::MAX as usize {
        return Err("macOS ImageIO input is empty or too large".to_string());
    }

    let api = ImageIoApi::load()?;
    let input_data =
        unsafe { (api.create_data)(std::ptr::null(), input.as_ptr(), input.len() as isize) };
    let _input_data = CfGuard::new(input_data, api.release, "input data")?;
    let source =
        unsafe { (api.create_source_with_data)(input_data, std::ptr::null(), std::ptr::null()) };
    let _source = CfGuard::new(source, api.release, "image source")?;
    let properties = unsafe { (api.copy_properties)(source, 0, std::ptr::null()) };
    let _properties = CfGuard::new(properties, api.release, "image properties")?;

    let width_number = unsafe { (api.dictionary_get_value)(properties, api.pixel_width_key) };
    let height_number = unsafe { (api.dictionary_get_value)(properties, api.pixel_height_key) };
    if width_number.is_null() || height_number.is_null() {
        return Err("macOS ImageIO did not return image dimensions".to_string());
    }
    let mut width = 0_i64;
    let mut height = 0_i64;
    let width_ok = unsafe {
        (api.number_get_value)(
            width_number,
            CF_NUMBER_SINT64_TYPE,
            (&mut width as *mut i64).cast(),
        )
    };
    let height_ok = unsafe {
        (api.number_get_value)(
            height_number,
            CF_NUMBER_SINT64_TYPE,
            (&mut height as *mut i64).cast(),
        )
    };
    if !width_ok || !height_ok || width <= 0 || height <= 0 {
        return Err("macOS ImageIO returned invalid image dimensions".to_string());
    }
    let width = u32::try_from(width)
        .map_err(|_| "macOS ImageIO image width exceeds the safe limit".to_string())?;
    let height = u32::try_from(height)
        .map_err(|_| "macOS ImageIO image height exceeds the safe limit".to_string())?;
    Ok((width, height))
}

/// 将受限读取的 HEIC/JPEG/PNG 字节编辑为适合桥接的静态 JPEG。
///
/// 这不是原图导出：ImageIO 会先按最长边 2600px 生成缩略图，再以 JPEG 0.85
/// 编码；调用方仍须对输入执行 no-follow、有界读取，并对返回字节继续做 data URL 限制。
pub(crate) fn encode_wallpaper_jpeg(
    input: &[u8],
    max_output_bytes: usize,
) -> Result<Vec<u8>, String> {
    if input.is_empty() {
        return Err("macOS ImageIO input is empty".to_string());
    }
    if input.len() > isize::MAX as usize || max_output_bytes == 0 {
        return Err("macOS ImageIO input or output limit is invalid".to_string());
    }

    let api = ImageIoApi::load()?;
    let input_data =
        unsafe { (api.create_data)(std::ptr::null(), input.as_ptr(), input.len() as isize) };
    let _input_data = CfGuard::new(input_data, api.release, "input data")?;

    let max_dimension = MAX_DIMENSION;
    let max_dimension_number = unsafe {
        (api.create_number)(
            std::ptr::null(),
            CF_NUMBER_SINT32_TYPE,
            (&max_dimension as *const i32).cast(),
        )
    };
    let _max_dimension_number =
        CfGuard::new(max_dimension_number, api.release, "thumbnail dimension")?;

    let source_keys = [
        api.thumbnail_from_image_always_key,
        api.thumbnail_max_pixel_size_key,
        api.thumbnail_with_transform_key,
    ];
    let source_values = [api.true_value, max_dimension_number, api.true_value];
    let source_options = unsafe {
        (api.create_dictionary)(
            std::ptr::null(),
            source_keys.as_ptr(),
            source_values.as_ptr(),
            source_keys.len() as isize,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    let _source_options = CfGuard::new(source_options, api.release, "thumbnail options")?;

    let source =
        unsafe { (api.create_source_with_data)(input_data, source_options, std::ptr::null()) };
    let _source = CfGuard::new(source, api.release, "image source")?;
    let image = unsafe { (api.create_thumbnail)(source, 0, source_options) };
    let _image = CfGuard::new(image, api.release, "thumbnail image")?;

    encode_cg_image(&api, image, max_output_bytes)
}

/// 调用方须保证 image 是存活的、已限制为最长边 2600px 的 CGImage。
pub(crate) unsafe fn encode_wallpaper_frame(
    image: *const c_void,
    max_output_bytes: usize,
) -> Result<Vec<u8>, String> {
    if image.is_null() {
        return Err("empty wallpaper frame".into());
    }
    let api = ImageIoApi::load()?;
    encode_cg_image(&api, image, max_output_bytes)
}

fn encode_cg_image(
    api: &ImageIoApi,
    image: CfRef,
    max_output_bytes: usize,
) -> Result<Vec<u8>, String> {
    let output_data = unsafe { (api.create_mutable_data)(std::ptr::null(), 0) };
    let _output_data = CfGuard::new(output_data.cast_const(), api.release, "output data")?;

    let jpeg_uti = CString::new("public.jpeg")
        .map_err(|_| "macOS ImageIO JPEG type is invalid".to_string())?;
    let jpeg_uti = unsafe {
        (api.create_string)(std::ptr::null(), jpeg_uti.as_ptr(), CF_STRING_ENCODING_UTF8)
    };
    let _jpeg_uti = CfGuard::new(jpeg_uti, api.release, "JPEG type")?;

    let quality = JPEG_QUALITY;
    let quality_number = unsafe {
        (api.create_number)(
            std::ptr::null(),
            CF_NUMBER_FLOAT64_TYPE,
            (&quality as *const f64).cast(),
        )
    };
    let _quality_number = CfGuard::new(quality_number, api.release, "JPEG quality")?;
    let destination_keys = [api.destination_quality_key];
    let destination_values = [quality_number];
    let destination_options = unsafe {
        (api.create_dictionary)(
            std::ptr::null(),
            destination_keys.as_ptr(),
            destination_values.as_ptr(),
            destination_keys.len() as isize,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    let _destination_options = CfGuard::new(destination_options, api.release, "JPEG options")?;

    let destination =
        unsafe { (api.create_destination)(output_data, jpeg_uti, 1, destination_options) };
    let _destination = CfGuard::new(destination.cast_const(), api.release, "JPEG destination")?;
    unsafe {
        (api.add_image)(destination, image, destination_options);
    }
    if !unsafe { (api.finalize)(destination) } {
        return Err("macOS ImageIO failed to finalize JPEG".to_string());
    }

    let output_length = unsafe { (api.data_get_length)(output_data.cast_const()) };
    if output_length <= 0 || output_length as usize > max_output_bytes {
        return Err("macOS ImageIO JPEG exceeds the safe output limit".to_string());
    }
    let output_ptr = unsafe { (api.data_get_byte_ptr)(output_data.cast_const()) };
    if output_ptr.is_null() {
        return Err("macOS ImageIO returned no JPEG data".to_string());
    }
    let output = unsafe { std::slice::from_raw_parts(output_ptr, output_length as usize).to_vec() };
    if !output.starts_with(b"\xff\xd8\xff") {
        return Err("macOS ImageIO did not produce a JPEG".to_string());
    }
    Ok(output)
}

struct ImageIoApi {
    _image_io: FrameworkHandle,
    _core_foundation: FrameworkHandle,
    release: ReleaseFn,
    create_data: CreateDataFn,
    create_mutable_data: CreateMutableDataFn,
    data_get_length: DataGetLengthFn,
    data_get_byte_ptr: DataGetBytePtrFn,
    create_number: CreateNumberFn,
    create_string: CreateStringFn,
    create_dictionary: CreateDictionaryFn,
    create_source_with_data: CreateSourceWithDataFn,
    #[cfg(test)]
    copy_properties: CopyPropertiesFn,
    create_thumbnail: CreateThumbnailFn,
    create_destination: CreateDestinationFn,
    add_image: AddImageFn,
    finalize: FinalizeFn,
    #[cfg(test)]
    dictionary_get_value: DictionaryGetValueFn,
    #[cfg(test)]
    number_get_value: NumberGetValueFn,
    true_value: CfRef,
    thumbnail_from_image_always_key: CfRef,
    thumbnail_max_pixel_size_key: CfRef,
    thumbnail_with_transform_key: CfRef,
    destination_quality_key: CfRef,
    #[cfg(test)]
    pixel_width_key: CfRef,
    #[cfg(test)]
    pixel_height_key: CfRef,
}

impl ImageIoApi {
    fn load() -> Result<Self, String> {
        let image_io = FrameworkHandle::open(
            "/System/Library/Frameworks/ImageIO.framework/Versions/A/ImageIO",
        )?;
        let core_foundation = FrameworkHandle::open(
            "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation",
        )?;

        let release = unsafe {
            std::mem::transmute::<*mut c_void, ReleaseFn>(core_foundation.symbol("CFRelease")?)
        };
        let create_data = unsafe {
            std::mem::transmute::<*mut c_void, CreateDataFn>(
                core_foundation.symbol("CFDataCreate")?,
            )
        };
        let create_mutable_data = unsafe {
            std::mem::transmute::<*mut c_void, CreateMutableDataFn>(
                core_foundation.symbol("CFDataCreateMutable")?,
            )
        };
        let data_get_length = unsafe {
            std::mem::transmute::<*mut c_void, DataGetLengthFn>(
                core_foundation.symbol("CFDataGetLength")?,
            )
        };
        let data_get_byte_ptr = unsafe {
            std::mem::transmute::<*mut c_void, DataGetBytePtrFn>(
                core_foundation.symbol("CFDataGetBytePtr")?,
            )
        };
        let create_number = unsafe {
            std::mem::transmute::<*mut c_void, CreateNumberFn>(
                core_foundation.symbol("CFNumberCreate")?,
            )
        };
        let create_string = unsafe {
            std::mem::transmute::<*mut c_void, CreateStringFn>(
                core_foundation.symbol("CFStringCreateWithCString")?,
            )
        };
        let create_dictionary = unsafe {
            std::mem::transmute::<*mut c_void, CreateDictionaryFn>(
                core_foundation.symbol("CFDictionaryCreate")?,
            )
        };
        let create_source_with_data = unsafe {
            std::mem::transmute::<*mut c_void, CreateSourceWithDataFn>(
                image_io.symbol("CGImageSourceCreateWithData")?,
            )
        };
        #[cfg(test)]
        let copy_properties = unsafe {
            std::mem::transmute::<*mut c_void, CopyPropertiesFn>(
                image_io.symbol("CGImageSourceCopyPropertiesAtIndex")?,
            )
        };
        let create_thumbnail = unsafe {
            std::mem::transmute::<*mut c_void, CreateThumbnailFn>(
                image_io.symbol("CGImageSourceCreateThumbnailAtIndex")?,
            )
        };
        let create_destination = unsafe {
            std::mem::transmute::<*mut c_void, CreateDestinationFn>(
                image_io.symbol("CGImageDestinationCreateWithData")?,
            )
        };
        let add_image = unsafe {
            std::mem::transmute::<*mut c_void, AddImageFn>(
                image_io.symbol("CGImageDestinationAddImage")?,
            )
        };
        let finalize = unsafe {
            std::mem::transmute::<*mut c_void, FinalizeFn>(
                image_io.symbol("CGImageDestinationFinalize")?,
            )
        };
        #[cfg(test)]
        let dictionary_get_value = unsafe {
            std::mem::transmute::<*mut c_void, DictionaryGetValueFn>(
                core_foundation.symbol("CFDictionaryGetValue")?,
            )
        };
        #[cfg(test)]
        let number_get_value = unsafe {
            std::mem::transmute::<*mut c_void, NumberGetValueFn>(
                core_foundation.symbol("CFNumberGetValue")?,
            )
        };
        let true_value = core_foundation.data_pointer("kCFBooleanTrue")?;
        let thumbnail_from_image_always_key =
            image_io.data_pointer("kCGImageSourceCreateThumbnailFromImageAlways")?;
        let thumbnail_max_pixel_size_key =
            image_io.data_pointer("kCGImageSourceThumbnailMaxPixelSize")?;
        let thumbnail_with_transform_key =
            image_io.data_pointer("kCGImageSourceCreateThumbnailWithTransform")?;
        let destination_quality_key =
            image_io.data_pointer("kCGImageDestinationLossyCompressionQuality")?;
        #[cfg(test)]
        let pixel_width_key = image_io.data_pointer("kCGImagePropertyPixelWidth")?;
        #[cfg(test)]
        let pixel_height_key = image_io.data_pointer("kCGImagePropertyPixelHeight")?;

        Ok(Self {
            _image_io: image_io,
            _core_foundation: core_foundation,
            release,
            create_data,
            create_mutable_data,
            data_get_length,
            data_get_byte_ptr,
            create_number,
            create_string,
            create_dictionary,
            create_source_with_data,
            #[cfg(test)]
            copy_properties,
            create_thumbnail,
            create_destination,
            add_image,
            finalize,
            #[cfg(test)]
            dictionary_get_value,
            #[cfg(test)]
            number_get_value,
            true_value,
            thumbnail_from_image_always_key,
            thumbnail_max_pixel_size_key,
            thumbnail_with_transform_key,
            destination_quality_key,
            #[cfg(test)]
            pixel_width_key,
            #[cfg(test)]
            pixel_height_key,
        })
    }
}

struct CfGuard {
    value: CfRef,
    release: ReleaseFn,
}

impl CfGuard {
    fn new(value: CfRef, release: ReleaseFn, label: &str) -> Result<Self, String> {
        if value.is_null() {
            return Err(format!("macOS ImageIO could not create {label}"));
        }
        Ok(Self { value, release })
    }
}

impl Drop for CfGuard {
    fn drop(&mut self) {
        unsafe { (self.release)(self.value) };
    }
}

struct FrameworkHandle(*mut c_void);

impl FrameworkHandle {
    fn open(path: &str) -> Result<Self, String> {
        let path = CString::new(path).map_err(|error| error.to_string())?;
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(dynamic_loader_error("cannot load macOS image framework"));
        }
        Ok(Self(handle))
    }

    fn symbol(&self, name: &str) -> Result<*mut c_void, String> {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        unsafe {
            libc::dlerror();
        }
        let symbol = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        if symbol.is_null() {
            return Err(dynamic_loader_error(&format!(
                "macOS image framework is missing symbol {}",
                name.to_string_lossy()
            )));
        }
        Ok(symbol)
    }

    fn data_pointer(&self, name: &str) -> Result<CfRef, String> {
        let symbol = self.symbol(name)?;
        let value = unsafe { *(symbol as *const CfRef) };
        if value.is_null() {
            return Err(format!("macOS image framework returned a null {name}"));
        }
        Ok(value)
    }
}

impl Drop for FrameworkHandle {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0);
        }
    }
}

fn dynamic_loader_error(prefix: &str) -> String {
    let error = unsafe { libc::dlerror() };
    if error.is_null() {
        return prefix.to_string();
    }
    let detail = unsafe { CStr::from_ptr(error) }.to_string_lossy();
    format!("{prefix}: {detail}")
}
