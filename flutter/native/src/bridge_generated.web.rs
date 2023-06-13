use super::*;
// Section: wire functions

#[wasm_bindgen]
pub fn wire_generate_qrcode(port_: MessagePort, amount: u8) {
    wire_generate_qrcode_impl(port_, amount)
}

#[wasm_bindgen]
pub fn wire_init_db(port_: MessagePort) {
    wire_init_db_impl(port_)
}

#[wasm_bindgen]
pub fn wire_get_balance(port_: MessagePort) {
    wire_get_balance_impl(port_)
}

#[wasm_bindgen]
pub fn wire_pay_invoice(port_: MessagePort, invoice: String) {
    wire_pay_invoice_impl(port_, invoice)
}

#[wasm_bindgen]
pub fn wire_import_token(port_: MessagePort, token: String) {
    wire_import_token_impl(port_, token)
}

// Section: allocate functions

// Section: related functions

// Section: impl Wire2Api

impl Wire2Api<String> for String {
    fn wire2api(self) -> String {
        self
    }
}

impl Wire2Api<Vec<u8>> for Box<[u8]> {
    fn wire2api(self) -> Vec<u8> {
        self.into_vec()
    }
}
// Section: impl Wire2Api for JsValue

impl Wire2Api<String> for JsValue {
    fn wire2api(self) -> String {
        self.as_string().expect("non-UTF-8 string, or not a string")
    }
}
impl Wire2Api<u8> for JsValue {
    fn wire2api(self) -> u8 {
        self.unchecked_into_f64() as _
    }
}
impl Wire2Api<Vec<u8>> for JsValue {
    fn wire2api(self) -> Vec<u8> {
        self.unchecked_into::<js_sys::Uint8Array>().to_vec().into()
    }
}
