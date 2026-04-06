#![allow(non_snake_case)]

use std::ffi::c_void;

use jni::errors::{Error, JniError};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::strings::{JNIStr, JNIString};
use jni::sys::{
    JavaVM as RawJavaVM, JNI_ERR, JNI_FALSE, JNI_TRUE, jboolean, jint, jlong, jstring,
};
use jni::vm::JavaVM;
use jni::{Env, EnvUnowned, NativeMethod, jni_sig, jni_str};

use crate::callbacks::{CandidateEvent, PreEditEvent};
use crate::context::ImeContext;
use crate::logger::{self, LogLevel};
use crate::model::{CandidateConfig, InputMode};
use crate::jvm;

const BIND_CLASS_PROPERTY: &str = "ingameime.jni.bind_class";

struct NativeMethodOwned {
    name: JNIString,
    sig: JNIString,
    fn_ptr: *const (),
}

impl NativeMethodOwned {
    fn new(name: &str, sig: &str, fn_ptr: *const ()) -> Self {
        Self {
            name: JNIString::new(name),
            sig: JNIString::new(sig),
            fn_ptr,
        }
    }

    fn as_native_method(&self) -> NativeMethod<'_> {
        unsafe {
            NativeMethod::from_raw_parts(
                self.name.borrowed(),
                self.sig.borrowed(),
                self.fn_ptr as *mut c_void,
            )
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: *mut RawJavaVM, _reserved: *mut c_void) -> jint {
    if vm.is_null() {
        return JNI_ERR as jint;
    }

    let java_vm = unsafe { JavaVM::from_raw(vm) };
    jvm::initialize(java_vm.clone());

    let registered: Result<(), jni::errors::Error> =
        java_vm.attach_current_thread(|env| register_native_methods(env));

    if registered.is_err() {
        return JNI_ERR as jint;
    }

    jni_sys::JNI_VERSION_1_8 as jint
}

fn register_native_methods(env: &mut Env<'_>) -> Result<(), jni::errors::Error> {
    let bind_class = resolve_bind_class(env)?;

    let callback_owner = bind_class;
    let commit_sig = format!("(JL{}$CommitCallback;)V", callback_owner);
    let preedit_sig = format!("(JL{}$PreEditCallback;)V", callback_owner);
    let candidate_sig = format!("(JL{}$CandidateListCallback;)V", callback_owner);
    let input_mode_sig = format!("(JL{}$InputModeCallback;)V", callback_owner);

    let methods_owned = vec![
        NativeMethodOwned::new(
            "rust_ime_library_create_input_context_win32",
            "(JIZ)J",
            rust_create_input_context_win32 as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_destroy_input_context",
            "(J)V",
            rust_destroy_input_context as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_input_context_activated",
            "(JZ)V",
            rust_set_input_context_activated as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_is_input_context_activated",
            "(J)Z",
            rust_is_input_context_activated as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_get_input_mode",
            "(JI)I",
            rust_get_input_mode as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_force_alpha_mode",
            "(J)V",
            rust_force_alpha_mode as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_force_native_mode",
            "(J)V",
            rust_force_native_mode as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_pre_edit_rect",
            "(JIIII)V",
            rust_set_pre_edit_rect as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_get_version",
            "()Ljava/lang/String;",
            rust_get_version as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_max_candidates",
            "(JI)V",
            rust_set_max_candidates as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_get_max_candidates",
            "(J)I",
            rust_get_max_candidates as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_debug_logging",
            "(Z)V",
            rust_set_debug_logging as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_init_logger",
            "(Ljava/lang/Object;)V",
            rust_init_logger as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_commit_callback",
            &commit_sig,
            rust_set_commit_callback as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_pre_edit_callback",
            &preedit_sig,
            rust_set_pre_edit_callback as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_candidate_list_callback",
            &candidate_sig,
            rust_set_candidate_list_callback as *const (),
        ),
        NativeMethodOwned::new(
            "rust_ime_library_set_input_mode_callback",
            &input_mode_sig,
            rust_set_input_mode_callback as *const (),
        ),
    ];

    let methods: Vec<NativeMethod<'_>> =
        methods_owned.iter().map(NativeMethodOwned::as_native_method).collect();

    let callback_owner_jni = JNIString::new(&callback_owner);
    unsafe {
        env.register_native_methods(callback_owner_jni.borrowed(), &methods)?;
    }

    logger::info(&format!(
        "JNI native methods registered for class: {}",
        callback_owner
    ));

    Ok(())
}

fn resolve_bind_class(env: &mut Env<'_>) -> Result<String, jni::errors::Error> {
    if let Some(raw) = read_system_property(env, BIND_CLASS_PROPERTY) {
        let normalized = normalize_class_name(&raw);
        if !normalized.is_empty() {
            return Ok(normalized);
        }
    }

    if let Some(raw) = option_env!("INGAMEIME_JNI_BIND_CLASS") {
        let normalized = normalize_class_name(raw);
        if !normalized.is_empty() {
            return Ok(normalized);
        }
    }

    logger::error(
        "Missing JNI bind class. Set Java system property 'ingameime.jni.bind_class' or compile with INGAMEIME_JNI_BIND_CLASS.",
    );
    Err(Error::JniCall(JniError::InvalidArguments))
}

fn read_system_property(env: &mut Env<'_>, key: &str) -> Option<String> {
    let key_obj = env.new_string(key).ok()?;
    let key_obj = JObject::from(key_obj);

    let value = env
        .call_static_method(
            jni_str!("java/lang/System"),
            jni_str!("getProperty"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
            &[JValue::Object(&key_obj)],
        )
        .ok()?;

    let value_obj = value.l().ok()?;
    if value_obj.is_null() {
        return None;
    }

    let value_jstring = unsafe { JString::from_raw(env, value_obj.as_raw()) };
    value_jstring.try_to_string(env).ok()
}

fn normalize_class_name(raw: &str) -> String {
    raw.trim().replace('.', "/")
}

extern "system" fn rust_create_input_context_win32(
    _env: EnvUnowned,
    _class: JClass,
    hwnd: jlong,
    api: jint,
    ui_less: jboolean,
) -> jlong {
    let ui_less = ui_less != JNI_FALSE;
    match ImeContext::create(hwnd as isize, api, ui_less) {
        Some(context) => Box::into_raw(Box::new(context)) as jlong,
        None => 0,
    }
}

extern "system" fn rust_destroy_input_context(_env: EnvUnowned, _class: JClass, ptr: jlong) {
    if ptr != 0 {
        unsafe {
            let _ = Box::from_raw(ptr as *mut ImeContext);
        }
    }
}

extern "system" fn rust_set_input_context_activated(
    _env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    activated: jboolean,
) {
    let Some(context) = context_mut(ptr) else {
        return;
    };
    context.set_activated(activated != JNI_FALSE);
}

extern "system" fn rust_is_input_context_activated(
    _env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    let Some(context) = context_ref(ptr) else {
        return JNI_FALSE;
    };

    if context.is_activated() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

extern "system" fn rust_get_input_mode(
    _env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    _legacy_mode: jint,
) -> jint {
    let Some(context) = context_ref(ptr) else {
        return 2;
    };

    match context.get_input_mode() {
        InputMode::Alpha => 0,
        InputMode::Native => 1,
        InputMode::Unsupported => 2,
    }
}

extern "system" fn rust_force_alpha_mode(_env: EnvUnowned, _class: JClass, ptr: jlong) {
    let Some(context) = context_mut(ptr) else {
        return;
    };
    context.force_alpha_mode();
}

extern "system" fn rust_force_native_mode(_env: EnvUnowned, _class: JClass, ptr: jlong) {
    let Some(context) = context_mut(ptr) else {
        return;
    };
    context.force_native_mode();
}

extern "system" fn rust_set_pre_edit_rect(
    _env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    x: jint,
    y: jint,
    width: jint,
    height: jint,
) {
    let Some(context) = context_mut(ptr) else {
        return;
    };
    context.set_preedit_rect(x, y, width, height);
}

extern "system" fn rust_get_version(mut env: EnvUnowned, _class: JClass) -> jstring {
    let mut output: jstring = JObject::null().into_raw();
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let value = env.new_string(env!("CARGO_PKG_VERSION"))?;
        output = value.into_raw();
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
    output
}

extern "system" fn rust_set_max_candidates(
    _env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    max_candidates: jint,
) {
    let Some(context) = context_mut(ptr) else {
        return;
    };

    let config = CandidateConfig {
        max_candidates: if max_candidates > 0 {
            max_candidates as usize
        } else {
            CandidateConfig::default().max_candidates
        },
    };
    context.set_candidate_config(config);
}

extern "system" fn rust_get_max_candidates(
    _env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
) -> jint {
    let Some(context) = context_ref(ptr) else {
        return CandidateConfig::default().max_candidates as jint;
    };
    context.candidate_config().max_candidates as jint
}

extern "system" fn rust_set_debug_logging(_env: EnvUnowned, _class: JClass, enabled: jboolean) {
    let enabled = enabled != JNI_FALSE;
    logger::set_debug(enabled);
    if enabled {
        logger::info("debug logging enabled");
    }
}

extern "system" fn rust_init_logger(mut env: EnvUnowned, _class: JClass, logger_obj: JObject) {
    if logger_obj.is_null() {
        return;
    }

    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let global = env.new_global_ref(&logger_obj)?;

        logger::set_log_hook(move |level, message| {
            let method: &'static JNIStr = match level {
                LogLevel::Error => jni_str!("error"),
                LogLevel::Warn => jni_str!("warn"),
                LogLevel::Debug => jni_str!("debug"),
                LogLevel::Info => jni_str!("info"),
            };

            jvm::with_attached_env(|env| -> Result<(), jni::errors::Error> {
                let jmsg = env.new_string(message)?;
                let jmsg_obj = JObject::from(jmsg);
                env.call_method(
                    global.as_obj(),
                    method,
                    jni_sig!("(Ljava/lang/String;)V"),
                    &[JValue::Object(&jmsg_obj)],
                )?;
                Ok(())
            });
        });

        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

extern "system" fn rust_set_commit_callback(
    mut env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    callback: JObject,
) {
    if callback.is_null() {
        return;
    }

    let Some(context) = context_mut(ptr) else {
        return;
    };

    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let global = env.new_global_ref(&callback)?;
        context.set_commit_callback(Box::new(move |text: String| {
            jvm::with_attached_env(|env| -> Result<(), jni::errors::Error> {
                let jtext = env.new_string(&text)?;
                let jtext_obj = JObject::from(jtext);
                env.call_method(
                    global.as_obj(),
                    jni_str!("onCommit"),
                    jni_sig!("(Ljava/lang/String;)V"),
                    &[JValue::Object(&jtext_obj)],
                )?;
                Ok(())
            });
        }));
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

extern "system" fn rust_set_pre_edit_callback(
    mut env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    callback: JObject,
) {
    if callback.is_null() {
        return;
    }

    let Some(context) = context_mut(ptr) else {
        return;
    };

    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let global = env.new_global_ref(&callback)?;
        context.set_preedit_callback(Box::new(move |event: PreEditEvent| {
            jvm::with_attached_env(|env| -> Result<(), jni::errors::Error> {
                match &event {
                    PreEditEvent::Begin => {
                        let null_obj = JObject::null();
                        env.call_method(
                            global.as_obj(),
                            jni_str!("onPreEdit"),
                            jni_sig!("(ILjava/lang/String;I)V"),
                            &[JValue::Int(0), JValue::Object(&null_obj), JValue::Int(-1)],
                        )?;
                    }
                    PreEditEvent::Update(preedit) => {
                        let jtext = env.new_string(&preedit.text)?;
                        let jtext_obj = JObject::from(jtext);
                        env.call_method(
                            global.as_obj(),
                            jni_str!("onPreEdit"),
                            jni_sig!("(ILjava/lang/String;I)V"),
                            &[
                                JValue::Int(1),
                                JValue::Object(&jtext_obj),
                                JValue::Int(preedit.cursor as jint),
                            ],
                        )?;
                    }
                    PreEditEvent::End => {
                        let null_obj = JObject::null();
                        env.call_method(
                            global.as_obj(),
                            jni_str!("onPreEdit"),
                            jni_sig!("(ILjava/lang/String;I)V"),
                            &[JValue::Int(2), JValue::Object(&null_obj), JValue::Int(-1)],
                        )?;
                    }
                }
                Ok(())
            });
        }));
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

extern "system" fn rust_set_candidate_list_callback(
    mut env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    callback: JObject,
) {
    if callback.is_null() {
        return;
    }

    let Some(context) = context_mut(ptr) else {
        return;
    };

    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let global = env.new_global_ref(&callback)?;
        context.set_candidate_callback(Box::new(move |event: CandidateEvent| {
            jvm::with_attached_env(|env| -> Result<(), jni::errors::Error> {
                match &event {
                    CandidateEvent::Begin => {
                        let null_obj = JObject::null();
                        env.call_method(
                            global.as_obj(),
                            jni_str!("onCandidateList"),
                            jni_sig!("(I[Ljava/lang/String;I)V"),
                            &[JValue::Int(0), JValue::Object(&null_obj), JValue::Int(-1)],
                        )?;
                    }
                    CandidateEvent::Update(candidate) => {
                        let arr = env.new_object_array(
                            candidate.candidates.len() as jint,
                            jni_str!("java/lang/String"),
                            JObject::null(),
                        )?;

                        for (idx, text) in candidate.candidates.iter().enumerate() {
                            let jtext = env.new_string(text)?;
                            arr.set_element(env, idx, &jtext)?;
                        }

                        let arr_obj = JObject::from(arr);
                        env.call_method(
                            global.as_obj(),
                            jni_str!("onCandidateList"),
                            jni_sig!("(I[Ljava/lang/String;I)V"),
                            &[
                                JValue::Int(1),
                                JValue::Object(&arr_obj),
                                JValue::Int(candidate.selected as jint),
                            ],
                        )?;
                    }
                    CandidateEvent::End => {
                        let null_obj = JObject::null();
                        env.call_method(
                            global.as_obj(),
                            jni_str!("onCandidateList"),
                            jni_sig!("(I[Ljava/lang/String;I)V"),
                            &[JValue::Int(2), JValue::Object(&null_obj), JValue::Int(-1)],
                        )?;
                    }
                }
                Ok(())
            });
        }));
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

extern "system" fn rust_set_input_mode_callback(
    mut env: EnvUnowned,
    _class: JClass,
    ptr: jlong,
    callback: JObject,
) {
    if callback.is_null() {
        return;
    }

    let Some(context) = context_mut(ptr) else {
        return;
    };

    env.with_env(|env| -> Result<(), jni::errors::Error> {
        let global = env.new_global_ref(&callback)?;
        context.set_input_mode_callback(Box::new(move |mode: InputMode| {
            jvm::with_attached_env(|env| -> Result<(), jni::errors::Error> {
                let mode_int: jint = match mode {
                    InputMode::Alpha => 0,
                    InputMode::Native => 1,
                    InputMode::Unsupported => 2,
                };
                env.call_method(
                    global.as_obj(),
                    jni_str!("onInputModeChanged"),
                    jni_sig!("(I)V"),
                    &[JValue::Int(mode_int)],
                )?;
                Ok(())
            });
        }));
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

fn context_ref<'a>(ptr: jlong) -> Option<&'a ImeContext> {
    if ptr == 0 {
        return None;
    }
    Some(unsafe { &*(ptr as *const ImeContext) })
}

fn context_mut<'a>(ptr: jlong) -> Option<&'a mut ImeContext> {
    if ptr == 0 {
        return None;
    }
    Some(unsafe { &mut *(ptr as *mut ImeContext) })
}


