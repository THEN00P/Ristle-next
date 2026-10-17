//! cpal's AAudio backend asks Java's AudioManager for burst sizes through
//! `ndk-context`, which a NativeActivity fills in but a UniFFI library does
//! not: JNA loads it with dlopen, so no JNI_OnLoad runs and the context stays
//! empty ("android context was not initialized"). Kotlin calls this once,
//! after `System.loadLibrary`, with the application context.

use jni_sys::{jobject, JNIEnv, JavaVM, JNI_OK};

#[no_mangle]
pub extern "system" fn Java_dev_ristle_spike_b_NativeContext_init(env: *mut JNIEnv, _class: jobject, context: jobject) {
    unsafe {
        let mut vm: *mut JavaVM = std::ptr::null_mut();
        let get_vm = (**env).GetJavaVM.expect("GetJavaVM");
        assert_eq!(get_vm(env, &mut vm), JNI_OK);
        let new_global = (**env).NewGlobalRef.expect("NewGlobalRef");
        let ctx = new_global(env, context);
        ndk_context::initialize_android_context(vm.cast(), ctx.cast());
    }
}
