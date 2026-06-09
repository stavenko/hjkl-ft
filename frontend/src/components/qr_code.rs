use fast_qr::convert::svg::SvgBuilder;
use fast_qr::convert::Builder;
use fast_qr::convert::Shape;
use fast_qr::qr::QRBuilder;
use leptos::*;

#[component]
pub fn QrCode(data: String, #[prop(default = 200)] size: u32) -> impl IntoView {
    let qrcode = QRBuilder::new(data).build().expect("Failed to build QR code");

    let svg_string = SvgBuilder::default()
        .shape(Shape::Square)
        .to_str(&qrcode);

    view! {
        <div
            style=format!("width: {}px; height: {}px;", size, size)
            inner_html=svg_string
        />
    }
}
