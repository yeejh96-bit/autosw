//! Minimal, thread-local Windows Automation client for Hancom.
//! https://learn.microsoft.com/windows/win32/api/oaidl/nf-oaidl-idispatch-invoke
//! https://learn.microsoft.com/windows/win32/learnwin32/initializing-the-com-library

use std::{
    ffi::{OsStr, c_void},
    marker::PhantomData,
    os::windows::ffi::OsStrExt,
    ptr::{NonNull, null, null_mut},
    rc::Rc,
};
use windows_sys::{
    Win32::{
        Foundation::{DISP_E_EXCEPTION, SysAllocStringLen, SysFreeString},
        Globalization::LOCALE_USER_DEFAULT,
        System::{
            Com::{
                CLSIDFromProgID, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
                CoCreateInstance, CoInitializeEx, CoUninitialize, DISPATCH_FLAGS, DISPATCH_METHOD,
                DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS, EXCEPINFO,
            },
            Ole::DISPID_PROPERTYPUT,
            Variant::{
                VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_BSTR, VT_DISPATCH,
                VT_I2, VT_I4, VT_INT, VT_UI4, VariantClear,
            },
        },
    },
    core::{GUID, HRESULT, PCWSTR},
};

const IID_NULL: GUID = GUID::from_u128(0);
const IID_DISPATCH: GUID = GUID::from_u128(0x00020400_0000_0000_c000_000000000046);

#[repr(C)]
struct DispatchVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_type_info_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    get_type_info: unsafe extern "system" fn(*mut c_void, u32, u32, *mut *mut c_void) -> HRESULT,
    get_ids_of_names: unsafe extern "system" fn(
        *mut c_void,
        *const GUID,
        *mut PCWSTR,
        u32,
        u32,
        *mut i32,
    ) -> HRESULT,
    invoke: unsafe extern "system" fn(
        *mut c_void,
        i32,
        *const GUID,
        u32,
        DISPATCH_FLAGS,
        *mut DISPPARAMS,
        *mut VARIANT,
        *mut EXCEPINFO,
        *mut u32,
    ) -> HRESULT,
}

fn check(operation: &str, result: HRESULT) -> Result<(), String> {
    if result < 0 {
        Err(format!("{operation}: COM 오류 0x{result:08X}"))
    } else {
        Ok(())
    }
}

pub(crate) struct Apartment(PhantomData<Rc<()>>);

impl Apartment {
    #[allow(unsafe_code)]
    pub(crate) fn initialize() -> Result<Self, String> {
        // Null is required for the reserved argument. Every success balances one drop.
        check("COM 초기화", unsafe {
            CoInitializeEx(
                null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        })?;
        Ok(Self(PhantomData))
    }
}

impl Drop for Apartment {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // This guard can only be dropped on its initializing thread.
        unsafe { CoUninitialize() };
    }
}

pub(crate) struct Dispatch(NonNull<c_void>);

impl Dispatch {
    #[allow(unsafe_code)]
    pub(crate) fn create(progid: &str, context: u32) -> Result<Self, String> {
        let name = wide_name(progid)?;
        let mut class = IID_NULL;
        // The ProgID is NUL-terminated and class is writable.
        check(progid, unsafe {
            CLSIDFromProgID(name.as_ptr(), &mut class)
        })?;
        let mut object = null_mut();
        // COM returns one owned IDispatch reference on success.
        check(progid, unsafe {
            CoCreateInstance(&class, null_mut(), context, &IID_DISPATCH, &mut object)
        })?;
        Self::from_pointer(object)
    }

    fn from_pointer(pointer: *mut c_void) -> Result<Self, String> {
        NonNull::new(pointer)
            .map(Self)
            .ok_or_else(|| "COM 객체가 비어 있습니다.".into())
    }

    #[allow(unsafe_code)]
    fn vtable(&self) -> &DispatchVtable {
        // Only CoCreateInstance or an owned VT_DISPATCH value can construct Self.
        unsafe { &**self.0.as_ptr().cast::<*const DispatchVtable>() }
    }

    pub(crate) fn call(&self, name: &str, args: &[Argument<'_>]) -> Result<Variant, String> {
        self.invoke(name, DISPATCH_METHOD, args)
    }

    pub(crate) fn get(&self, name: &str, args: &[Argument<'_>]) -> Result<Variant, String> {
        self.invoke(name, DISPATCH_PROPERTYGET, args)
    }

    pub(crate) fn put(&self, name: &str, value: Argument<'_>) -> Result<(), String> {
        self.invoke(name, DISPATCH_PROPERTYPUT, &[value]).map(drop)
    }

    #[allow(unsafe_code)]
    fn invoke(
        &self,
        name: &str,
        flags: DISPATCH_FLAGS,
        args: &[Argument<'_>],
    ) -> Result<Variant, String> {
        let name_wide = wide_name(name)?;
        let mut name_pointer = name_wide.as_ptr();
        let mut id = 0;
        let object = self.0.as_ptr();
        let table = self.vtable();
        // The method name and writable ID live across GetIDsOfNames.
        check(name, unsafe {
            (table.get_ids_of_names)(
                object,
                &IID_NULL,
                &mut name_pointer,
                1,
                LOCALE_USER_DEFAULT,
                &mut id,
            )
        })?;
        let mut named = DISPID_PROPERTYPUT;
        // COM reads positional arguments in reverse order. Each Variant owns its payload.
        let mut arguments = args
            .iter()
            .rev()
            .map(Variant::from_argument)
            .collect::<Result<Vec<_>, _>>()?;
        let property_put = flags == DISPATCH_PROPERTYPUT;
        let mut parameters = DISPPARAMS {
            rgvarg: if arguments.is_empty() {
                null_mut()
            } else {
                arguments.as_mut_ptr().cast()
            },
            rgdispidNamedArgs: if property_put { &mut named } else { null_mut() },
            cArgs: u32::try_from(arguments.len()).map_err(|_| "COM 인자가 너무 많습니다.")?,
            cNamedArgs: u32::from(property_put),
        };
        let mut result = Variant(VARIANT::default());
        let mut exception = EXCEPINFO::default();
        // Arguments and the initialized result are valid for this synchronous call.
        let status = unsafe {
            (table.invoke)(
                object,
                id,
                &IID_NULL,
                LOCALE_USER_DEFAULT,
                flags,
                &mut parameters,
                &mut result.0,
                &mut exception,
                null_mut(),
            )
        };
        // An automation exception can carry an underlying HRESULT. Free all owned
        // BSTRs here and keep exception messages and paths out of diagnostics.
        unsafe {
            if status == DISP_E_EXCEPTION
                && let Some(fill) = exception.pfnDeferredFillIn
            {
                fill(&mut exception);
            }
            SysFreeString(exception.bstrSource);
            SysFreeString(exception.bstrDescription);
            SysFreeString(exception.bstrHelpFile);
        }
        check(
            name,
            if status == DISP_E_EXCEPTION && exception.scode < 0 {
                exception.scode
            } else {
                status
            },
        )?;
        Ok(result)
    }
}

impl Drop for Dispatch {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // Release exactly the reference owned by this wrapper.
        unsafe { (self.vtable().release)(self.0.as_ptr()) };
    }
}

fn wide_name(value: &str) -> Result<Vec<u16>, String> {
    if value.contains('\0') {
        return Err("COM 이름에 NUL 문자를 사용할 수 없습니다.".into());
    }
    Ok(value.encode_utf16().chain([0]).collect())
}

pub(crate) enum Argument<'a> {
    Text(&'a OsStr),
    I32(i32),
    Bool(bool),
}

#[repr(transparent)]
pub(crate) struct Variant(VARIANT);

impl Variant {
    #[allow(unsafe_code)]
    fn from_argument(argument: &Argument<'_>) -> Result<Self, String> {
        let (vt, value) = match *argument {
            Argument::Text(text) => {
                let utf16: Vec<_> = text.encode_wide().collect();
                let length = u32::try_from(utf16.len()).map_err(|_| "COM 문자열이 너무 깁니다.")?;
                // SysAllocStringLen copies the counted UTF-16 buffer into an owned BSTR.
                let bstr = unsafe { SysAllocStringLen(utf16.as_ptr(), length) };
                if bstr.is_null() {
                    return Err("COM 문자열을 할당하지 못했습니다.".into());
                }
                (VT_BSTR, VARIANT_0_0_0 { bstrVal: bstr })
            }
            Argument::I32(value) => (VT_I4, VARIANT_0_0_0 { lVal: value }),
            Argument::Bool(value) => (
                VT_BOOL,
                VARIANT_0_0_0 {
                    boolVal: -i16::from(value),
                },
            ),
        };
        Ok(Self(VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: VARIANT_0_0 {
                    vt,
                    Anonymous: value,
                    ..VARIANT_0_0::default()
                },
            },
        }))
    }

    #[allow(unsafe_code)]
    pub(crate) fn object(mut self) -> Result<Dispatch, String> {
        // Inspect the tag before reading the union and move ownership out of the Variant.
        let value = unsafe { self.0.Anonymous.Anonymous };
        if value.vt != VT_DISPATCH {
            return Err(format!("COM 객체 값이 필요합니다. 형식 {}", value.vt));
        }
        let pointer = unsafe { value.Anonymous.pdispVal };
        self.0 = VARIANT::default();
        Dispatch::from_pointer(pointer)
    }

    #[allow(unsafe_code)]
    pub(crate) fn integer(&self) -> Result<i32, String> {
        let value = unsafe { self.0.Anonymous.Anonymous };
        // Read only the integer field matching the COM type tag.
        unsafe {
            match value.vt {
                VT_I2 => Ok(i32::from(value.Anonymous.iVal)),
                VT_I4 => Ok(value.Anonymous.lVal),
                VT_INT => Ok(value.Anonymous.intVal),
                VT_UI4 => Ok(value.Anonymous.ulVal as i32),
                _ => Err(format!("COM 정수 값이 필요합니다. 형식 {}", value.vt)),
            }
        }
    }

    #[allow(unsafe_code)]
    pub(crate) fn boolean(&self) -> Result<bool, String> {
        let value = unsafe { self.0.Anonymous.Anonymous };
        if value.vt != VT_BOOL {
            return Err(format!("COM 참/거짓 값이 필요합니다. 형식 {}", value.vt));
        }
        // VARIANT_TRUE uses -1 and VARIANT_FALSE uses zero.
        Ok(unsafe { value.Anonymous.boolVal } != 0)
    }
}

impl Drop for Variant {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // The initialized VARIANT owns any BSTR or IDispatch payload.
        unsafe { VariantClear(&mut self.0) };
    }
}
