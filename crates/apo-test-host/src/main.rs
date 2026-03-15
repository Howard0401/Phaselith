// APO Test Host
//
// Loads the Phaselith APO DLL directly via LoadLibrary and tests the COM interfaces
// without needing audiodg.exe or registry bindings.
// Simulates the exact call sequence that audiodg uses.

#[cfg(windows)]
fn main() {
    use windows::core::*;
    use windows::Win32::System::LibraryLoader::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::Media::Audio::Apo::*;

    // APO CLSID
    const CLSID_PHASELITH_APO: GUID =
        GUID::from_u128(0xA1B2C3D4_E5F6_4A5B_9C8D_1E2F3A4B5C6D);

    let dll_path = std::env::current_exe()
        .unwrap()
        .parent().unwrap()
        .join("phaselith_apo.dll");

    println!("=== APO Test Host ===");
    println!("DLL path: {}", dll_path.display());

    if !dll_path.exists() {
        println!("ERROR: DLL not found at {}", dll_path.display());
        std::process::exit(1);
    }

    unsafe {
        // Step 1: Load DLL
        println!("\n[1] Loading DLL...");
        let dll_path_wide: Vec<u16> = dll_path.to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let hmodule = LoadLibraryW(PCWSTR(dll_path_wide.as_ptr()));
        match hmodule {
            Ok(h) => println!("    OK: HMODULE = {:?}", h),
            Err(e) => {
                println!("    FAILED: {}", e);
                std::process::exit(1);
            }
        }
        let hmodule = hmodule.unwrap();

        // Step 2: Get DllGetClassObject
        println!("\n[2] Getting DllGetClassObject...");
        type DllGetClassObjectFn = unsafe extern "system" fn(
            *const GUID, *const GUID, *mut *mut core::ffi::c_void,
        ) -> HRESULT;

        let proc = GetProcAddress(hmodule, PCSTR(b"DllGetClassObject\0".as_ptr()));
        if proc.is_none() {
            println!("    FAILED: DllGetClassObject not found");
            std::process::exit(1);
        }
        let get_class_object: DllGetClassObjectFn = std::mem::transmute(proc.unwrap());
        println!("    OK: function found");

        // Step 3: Get IClassFactory
        println!("\n[3] Getting IClassFactory...");
        let mut factory_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = get_class_object(
            &CLSID_PHASELITH_APO,
            &IClassFactory::IID,
            &mut factory_ptr,
        );
        if hr.is_err() {
            println!("    FAILED: HRESULT = {:?}", hr);
            std::process::exit(1);
        }
        let factory: IClassFactory = IClassFactory::from_raw(factory_ptr);
        println!("    OK: IClassFactory obtained");

        // Step 4: Create APO instance
        println!("\n[4] Creating APO instance...");
        let apo_unk: IUnknown = match factory.CreateInstance::<_, IUnknown>(None) {
            Ok(obj) => obj,
            Err(e) => {
                println!("    FAILED: {}", e);
                std::process::exit(1);
            }
        };
        println!("    OK: APO instance created");

        // Step 5: QueryInterface for IAudioProcessingObject
        println!("\n[5] QueryInterface for IAudioProcessingObject...");
        let apo: IAudioProcessingObject = match apo_unk.cast() {
            Ok(a) => a,
            Err(e) => {
                println!("    FAILED: {}", e);
                std::process::exit(1);
            }
        };
        println!("    OK");

        // Step 6: GetRegistrationProperties
        println!("\n[6] GetRegistrationProperties...");
        match apo.GetRegistrationProperties() {
            Ok(props) => {
                let p = &*props;
                println!("    CLSID: {:?}", p.clsid);
                println!("    Flags: {:?}", p.Flags);
                let name = String::from_utf16_lossy(
                    &p.szFriendlyName[..p.szFriendlyName.iter().position(|&c| c == 0).unwrap_or(0)]
                );
                println!("    Name: {}", name);
                CoTaskMemFree(Some(props as *const _ as *const _));
            }
            Err(e) => println!("    FAILED: {}", e),
        }

        // Step 7: Initialize
        println!("\n[7] Initialize...");
        match apo.Initialize(&[]) {
            Ok(()) => println!("    OK"),
            Err(e) => println!("    FAILED: {}", e),
        }

        // Step 8: QueryInterface for IAudioProcessingObjectConfiguration
        println!("\n[8] QueryInterface for IAudioProcessingObjectConfiguration...");
        let apo_config: IAudioProcessingObjectConfiguration = match apo_unk.cast() {
            Ok(a) => a,
            Err(e) => {
                println!("    FAILED: {}", e);
                std::process::exit(1);
            }
        };
        println!("    OK");

        // Step 9: QueryInterface for IAudioProcessingObjectRT
        println!("\n[9] QueryInterface for IAudioProcessingObjectRT...");
        let apo_rt: IAudioProcessingObjectRT = match apo_unk.cast() {
            Ok(a) => a,
            Err(e) => {
                println!("    FAILED: {}", e);
                std::process::exit(1);
            }
        };
        println!("    OK");

        // Step 10: LockForProcess
        println!("\n[10] LockForProcess (empty descriptors)...");
        let input_descs: &[*const APO_CONNECTION_DESCRIPTOR] = &[];
        let output_descs: &[*const APO_CONNECTION_DESCRIPTOR] = &[];
        match apo_config.LockForProcess(
            input_descs,
            output_descs,
        ) {
            Ok(()) => println!("    OK"),
            Err(e) => println!("    FAILED: {}", e),
        }

        // Step 11: APOProcess test
        println!("\n[11] APOProcess (480 frames stereo)...");
        let frames = 480usize;
        let sample_count = frames * 2; // stereo
        let mut input_buf = vec![0.0f32; sample_count];
        let mut output_buf = vec![0.0f32; sample_count];

        // Put a small signal in the input
        for i in 0..sample_count {
            input_buf[i] = (i as f32 / sample_count as f32) * 0.1;
        }

        let input_prop = APO_CONNECTION_PROPERTY {
            pBuffer: input_buf.as_ptr() as usize,
            u32ValidFrameCount: frames as u32,
            u32BufferFlags: APO_BUFFER_FLAGS(1), // BUFFER_VALID
            u32Signature: 0,
        };
        let mut output_prop = APO_CONNECTION_PROPERTY {
            pBuffer: output_buf.as_mut_ptr() as usize,
            u32ValidFrameCount: 0,
            u32BufferFlags: APO_BUFFER_FLAGS(0),
            u32Signature: 0,
        };

        let input_ptrs: [*const APO_CONNECTION_PROPERTY; 1] = [&input_prop];
        let mut output_ptrs: [*mut APO_CONNECTION_PROPERTY; 1] = [&mut output_prop];

        apo_rt.APOProcess(
            1,
            input_ptrs.as_ptr(),
            1,
            output_ptrs.as_mut_ptr(),
        );

        println!("    Output flags: {:?}", output_prop.u32BufferFlags);
        println!("    Output frames: {}", output_prop.u32ValidFrameCount);
        println!("    First 8 output samples: {:?}", &output_buf[..8]);

        let has_output = output_buf.iter().any(|&s| s != 0.0);
        println!("    Has non-zero output: {}", has_output);

        // Step 12: Multiple process calls
        println!("\n[12] Running 100 APOProcess calls...");
        for i in 0..100 {
            for j in 0..frames {
                let t = (i * frames + j) as f32 / 48000.0;
                let sample = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5;
                input_buf[j * 2] = sample;
                input_buf[j * 2 + 1] = sample;
            }

            let input_prop = APO_CONNECTION_PROPERTY {
                pBuffer: input_buf.as_ptr() as usize,
                u32ValidFrameCount: frames as u32,
                u32BufferFlags: APO_BUFFER_FLAGS(1),
                u32Signature: 0,
            };
            let mut output_prop = APO_CONNECTION_PROPERTY {
                pBuffer: output_buf.as_mut_ptr() as usize,
                u32ValidFrameCount: 0,
                u32BufferFlags: APO_BUFFER_FLAGS(0),
                u32Signature: 0,
            };

            let input_ptrs: [*const APO_CONNECTION_PROPERTY; 1] = [&input_prop];
            let mut output_ptrs: [*mut APO_CONNECTION_PROPERTY; 1] = [&mut output_prop];

            apo_rt.APOProcess(
                1,
                input_ptrs.as_ptr(),
                1,
                output_ptrs.as_mut_ptr(),
            );
        }
        println!("    OK: 100 calls completed without crash");

        // Step 13: UnlockForProcess
        println!("\n[13] UnlockForProcess...");
        match apo_config.UnlockForProcess() {
            Ok(()) => println!("    OK"),
            Err(e) => println!("    FAILED: {}", e),
        }

        println!("\n=== ALL TESTS PASSED ===");

        // Print APO debug log
        println!("\n=== APO Debug Log ===");
        if let Ok(log) = std::fs::read_to_string("C:\\ProgramData\\Phaselith\\apo_debug.log") {
            for line in log.lines().rev().take(30).collect::<Vec<_>>().into_iter().rev() {
                println!("  {}", line);
            }
        }
    }
}

#[cfg(not(windows))]
fn main() {
    println!("This test host only runs on Windows");
}
