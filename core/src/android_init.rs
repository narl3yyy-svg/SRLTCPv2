//! Android JNI bootstrap for iroh DNS (`ndk_context`).
//!
//! iroh reads system DNS via JNI and requires `ndk_context` to be set before
//! any Endpoint is built. With workspace `panic = "abort"`, a missing context
//! aborts the process instead of falling back to Google DNS.

use std::ffi::c_void;
use std::sync::Once;

static INIT: Once = Once::new();

/// Called from Kotlin: `AndroidInit.install(applicationContext)`.
///
/// # Safety
/// JNI entrypoint. We store a process-lifetime global ref to the Application context.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_srltcp_v2_AndroidInit_install(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    context: jni::objects::JObject,
) {
    INIT.call_once(|| match install_inner(&mut env, context) {
        Ok(()) => android_log("AndroidInit.install: ndk_context ready for iroh"),
        Err(e) => android_log(&format!("AndroidInit.install FAILED: {e}")),
    });
}

#[cfg(target_os = "android")]
fn install_inner(
    env: &mut jni::JNIEnv,
    context: jni::objects::JObject,
) -> Result<(), String> {
    // Application Context must outlive all DNS/TLS JNI calls.
    let global = env
        .new_global_ref(context)
        .map_err(|e| format!("global ref: {e}"))?;
    let ctx_ptr = global.as_raw() as *mut c_void;
    std::mem::forget(global);

    let vm = env
        .get_java_vm()
        .map_err(|e| format!("get_java_vm: {e}"))?;
    let vm_ptr = vm.get_java_vm_pointer() as *mut c_void;

    unsafe {
        iroh::dns::install_android_jni_context(vm_ptr, ctx_ptr);
    }
    Ok(())
}

#[cfg(target_os = "android")]
fn android_log(msg: &str) {
    use std::ffi::CString;
    extern "C" {
        fn __android_log_write(prio: i32, tag: *const i8, text: *const i8) -> i32;
    }
    const ANDROID_LOG_INFO: i32 = 4;
    if let (Ok(tag), Ok(text)) = (CString::new("SrltcpAndroidInit"), CString::new(msg)) {
        unsafe {
            __android_log_write(
                ANDROID_LOG_INFO,
                tag.as_ptr() as *const i8,
                text.as_ptr() as *const i8,
            );
        }
    }
}
