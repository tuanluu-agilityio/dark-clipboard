//! All user-visible strings (GC-14). Every message exists in English and Vietnamese;
//! the `messages!` macro makes a missing language a compile error.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Vi,
}

/// `messages! { Variant => ("english", "vietnamese"), ... }` generates `enum Msg`,
/// `Msg::ALL` and `fn t(lang, msg) -> &'static str`.
macro_rules! messages {
    ($($variant:ident => ($en:expr, $vi:expr)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Msg { $($variant),+ }

        impl Msg {
            pub const ALL: &'static [Msg] = &[$(Msg::$variant),+];
        }

        /// Looks up the text of `msg` in `lang`.
        pub fn t(lang: Lang, msg: Msg) -> &'static str {
            match (lang, msg) {
                $(
                    (Lang::En, Msg::$variant) => $en,
                    (Lang::Vi, Msg::$variant) => $vi,
                )+
            }
        }
    };
}

messages! {
    // Sensitive categories
    CategoryApiKey => ("API key", "Khóa API"),
    CategoryToken => ("Token", "Mã thông báo"),
    CategoryPrivateKey => ("Private key", "Khóa riêng tư"),
    CategoryPassword => ("Password", "Mật khẩu"),
    CategoryPaymentCard => ("Payment card", "Thẻ thanh toán"),
    CategoryEmail => ("Email", "Email"),
    CategoryPhone => ("Phone number", "Số điện thoại"),
    // Clip types
    ClipTypeText => ("Text", "Văn bản"),
    ClipTypeRichText => ("Rich text", "Văn bản định dạng"),
    ClipTypeUrl => ("Link", "Liên kết"),
    ClipTypeColor => ("Color", "Màu sắc"),
    ClipTypeCode => ("Code", "Mã nguồn"),
    ClipTypeImage => ("Image", "Hình ảnh"),
    ClipTypeFiles => ("Files", "Tệp"),
    // API error codes (generic; never include clip content)
    ErrNotFound => ("Not found", "Không tìm thấy"),
    ErrInvalidArgument => ("Invalid argument", "Tham số không hợp lệ"),
    ErrPermissionDenied => ("Permission denied", "Không có quyền truy cập"),
    ErrUnsupportedPlatform => (
        "Not supported on this platform",
        "Không được hỗ trợ trên nền tảng này"
    ),
    ErrClipboardBusy => ("The clipboard is busy", "Bảng nhớ tạm đang bận"),
    ErrStorageError => ("Storage error", "Lỗi lưu trữ"),
    ErrAppNotRunning => ("Dark Clipboard is not running", "Dark Clipboard chưa chạy"),
    ErrSchemaTooNew => (
        "Data was created by a newer version",
        "Dữ liệu được tạo bởi phiên bản mới hơn"
    ),
    ErrInternal => ("Internal error", "Lỗi nội bộ"),
    // Command line (headless dev binary)
    CliUsage => (
        "Usage: dark-clipboard --headless [--data-dir <path>]
The windowed app arrives in a later release.",
        "Cách dùng: dark-clipboard --headless [--data-dir <đường dẫn>]
Ứng dụng có cửa sổ sẽ có ở bản sau."
    ),
    CliNotSupportedYet => (
        "Dark Clipboard does not support this operating system yet.",
        "Dark Clipboard chưa hỗ trợ hệ điều hành này."
    ),
    CliStartFailed => (
        "Dark Clipboard could not start.",
        "Dark Clipboard không thể khởi động."
    ),
    CliRunning => (
        "Dark Clipboard is running headless. Press Ctrl+C to stop.",
        "Dark Clipboard đang chạy không giao diện. Nhấn Ctrl+C để dừng."
    ),
}

/// Messages allowed to read the same in both languages.
pub const SAME_IN_BOTH: &[Msg] = &[Msg::CategoryEmail];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i18n_complete() {
        for &m in Msg::ALL {
            let en = t(Lang::En, m);
            let vi = t(Lang::Vi, m);
            assert!(!en.is_empty(), "{m:?} en empty");
            assert!(!vi.is_empty(), "{m:?} vi empty");
            if !SAME_IN_BOTH.contains(&m) {
                assert_ne!(en, vi, "{m:?} identical in both languages");
            }
        }
    }
}
