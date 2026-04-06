use std::sync::OnceLock;

use jni::{Env, JavaVM};

static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

pub fn initialize(vm: JavaVM) {
    let _ = JAVA_VM.set(vm);
}

pub fn with_attached_env<F>(f: F)
where
    F: FnOnce(&mut Env<'_>) -> jni::errors::Result<()>,
{
    let Some(vm) = JAVA_VM.get() else {
        return;
    };

    let _: Result<(), jni::errors::Error> = vm.attach_current_thread(|env| f(env));
}
