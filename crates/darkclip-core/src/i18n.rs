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
        "Usage: dark-clipboard [--headless | --toggle | --quit] [--data-dir <path>]
  (no flag)   start the app hidden
  --headless  capture without a window, until Ctrl+C
  --toggle    ask the running app to show or hide the popup
  --quit      ask the running app to quit",
        "Cách dùng: dark-clipboard [--headless | --toggle | --quit] [--data-dir <đường dẫn>]
  (không cờ)  chạy ứng dụng ở chế độ ẩn
  --headless  ghi nhớ không cửa sổ, đến khi nhấn Ctrl+C
  --toggle    yêu cầu ứng dụng đang chạy hiện hoặc ẩn cửa sổ nhanh
  --quit      yêu cầu ứng dụng đang chạy thoát"
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
    // Desktop window (Task 201)
    UiAppName => ("Dark Clipboard", "Dark Clipboard"),
    UiPopupPlaceholder => (
        "Your clipboard history will appear here.",
        "Lịch sử bảng nhớ tạm của bạn sẽ hiện ở đây."
    ),
    UiCaptureActive => ("Capture is on", "Đang ghi nhớ"),
    UiCapturePaused => ("Capture is paused", "Đã tạm dừng ghi nhớ"),
    UiCapturePrivate => ("Private mode: nothing is saved", "Chế độ riêng tư: không lưu gì"),
    UiCaptureUnavailable => ("Capture is unavailable", "Không thể ghi nhớ"),
    UiStarting => ("Starting...", "Đang khởi động..."),
    // Popup (Task 204)
    UiSearchPlaceholder => ("Search your clipboard history", "Tìm trong lịch sử bảng nhớ tạm"),
    UiSearchLabel => ("Search", "Tìm kiếm"),
    UiFiltersLabel => ("Filters", "Bộ lọc"),
    UiClipListLabel => ("Clipboard history", "Lịch sử bảng nhớ tạm"),
    UiFilterPinned => ("Pinned", "Đã ghim"),
    UiFilterToday => ("Today", "Hôm nay"),
    UiFilter7Days => ("7 days", "7 ngày"),
    UiEmptyHistory => (
        "Nothing here yet. Copy some text and it will show up here.",
        "Chưa có gì. Hãy sao chép một đoạn văn bản, nó sẽ hiện ở đây."
    ),
    UiNoResults => ("No matching clips", "Không có mục nào phù hợp"),
    UiLoading => ("Loading...", "Đang tải..."),
    UiLoadFailed => (
        "The history could not be loaded",
        "Không tải được lịch sử"
    ),
    UiActionFailed => (
        "That did not work. Please try again.",
        "Không thực hiện được. Hãy thử lại."
    ),
    UiCopy => ("Copy", "Sao chép"),
    UiCopyPlain => ("Copy as plain text", "Sao chép dạng văn bản thuần"),
    UiPin => ("Pin", "Ghim"),
    UiUnpin => ("Unpin", "Bỏ ghim"),
    UiDelete => ("Delete", "Xóa"),
    UiDeleted => ("Clip deleted", "Đã xóa mục"),
    UiUndo => ("Undo", "Hoàn tác"),
    UiClose => ("Close", "Đóng"),
    UiKeyHints => (
        "Enter copy · Shift+Enter plain · Ctrl+P pin · Del delete · → details · Esc close",
        "Enter sao chép · Shift+Enter văn bản thuần · Ctrl+P ghim · Del xóa · → chi tiết · Esc đóng"
    ),
    // Copy back and paste (Task 205)
    UiKeyHintsPaste => (
        "Enter copy · Ctrl+Enter paste · Shift+Enter plain · Ctrl+P pin · Del delete · → details · Esc close",
        "Enter sao chép · Ctrl+Enter dán · Shift+Enter văn bản thuần · Ctrl+P ghim · Del xóa · → chi tiết · Esc đóng"
    ),
    UiKeyHintsEnterPastes => (
        "Enter paste · Shift+Enter plain · Ctrl+P pin · Del delete · → details · Esc close",
        "Enter dán · Shift+Enter văn bản thuần · Ctrl+P ghim · Del xóa · → chi tiết · Esc đóng"
    ),
    UiPasteManual => ("Copied — paste manually", "Đã sao chép — hãy tự dán"),
    UiPasteElevated => (
        "Copied — that app runs as administrator, so Dark Clipboard cannot paste into it. Paste manually.",
        "Đã sao chép — ứng dụng đó chạy với quyền quản trị nên Dark Clipboard không thể dán vào. Hãy tự dán."
    ),
    // Previews and item detail (Task 206)
    UiTimeNow => ("just now", "vừa xong"),
    UiTimeMinutes => ("{n} min ago", "{n} phút trước"),
    UiTimeHours => ("{n} h ago", "{n} giờ trước"),
    UiTimeDays => ("{n} d ago", "{n} ngày trước"),
    UiMaskedBadge => ("Masked", "Đã che"),
    UiMaskedDetail => (
        "Hidden because it may contain a secret.",
        "Đã ẩn vì có thể chứa thông tin bí mật."
    ),
    UiDetailLabel => ("Details", "Chi tiết"),
    UiLoadFullText => ("Load full text", "Tải toàn bộ văn bản"),
    UiTextCut => (
        "Showing the first {n} KiB only.",
        "Chỉ hiển thị {n} KiB đầu tiên."
    ),
    UiLoadLargerImage => ("Load larger preview", "Tải ảnh xem trước lớn hơn"),
    UiNoImagePreview => ("No preview image stored", "Không có ảnh xem trước"),
    UiFilesCount => ("{n} files", "{n} tệp"),
    UiFilesMore => ("and {n} more", "và {n} tệp khác"),
    // History window (Task 211)
    UiHistoryTitle => ("Dark Clipboard - History", "Dark Clipboard - Lịch sử"),
    UiFilterAll => ("All", "Tất cả"),
    UiFilter30Days => ("30 days", "30 ngày"),
    UiGroupType => ("Type", "Loại"),
    UiGroupDate => ("Date", "Ngày"),
    UiGroupSource => ("Source app", "Ứng dụng nguồn"),
    UiSelectedCount => ("{n} selected", "Đã chọn {n}"),
    UiSelectAll => ("Select all", "Chọn tất cả"),
    UiClearSelection => ("Clear selection", "Bỏ chọn"),
    UiCancel => ("Cancel", "Hủy"),
    UiConfirmDeleteOne => (
        "Delete this clip? This cannot be undone.",
        "Xóa mục này? Không thể hoàn tác."
    ),
    UiConfirmDeleteMany => (
        "Delete {n} clips? This cannot be undone.",
        "Xóa {n} mục? Không thể hoàn tác."
    ),
    UiDeletedMany => ("{n} clips deleted", "Đã xóa {n} mục"),
    UiKeyHintsHistory => (
        "Enter copy · Shift+Enter plain · Ctrl+P pin · Del delete · Ctrl+A select all · Esc close",
        "Enter sao chép · Shift+Enter văn bản thuần · Ctrl+P ghim · Del xóa · Ctrl+A chọn tất cả · Esc đóng"
    ),
    UiCopied => ("Copied to the clipboard", "Đã sao chép vào bảng nhớ tạm"),
    UiKeyHintsConfirm => ("Enter delete · Esc cancel", "Enter xóa · Esc hủy"),
    UiHistoryHint => (
        "Select one clip to see its details.",
        "Chọn một mục để xem chi tiết."
    ),
    CliSeedRefused => (
        "--seed-demo-clips needs --data-dir with a folder other than the default data folder.",
        "--seed-demo-clips cần --data-dir với một thư mục khác thư mục dữ liệu mặc định."
    ),
    CliSeedDone => ("Demo clips written:", "Đã ghi các mục mẫu:"),
    // Tray icon and menu (Task 202)
    TrayOpen => ("Open", "Mở"),
    TrayHistory => ("History...", "Lịch sử..."),
    TrayPause => ("Pause capture", "Tạm dừng ghi nhớ"),
    TrayPause5Min => ("5 minutes", "5 phút"),
    TrayPause1Hour => ("1 hour", "1 giờ"),
    TrayPauseUntilResumed => ("Until resumed", "Đến khi bật lại"),
    TrayResume => ("Resume capture", "Tiếp tục ghi nhớ"),
    TrayPrivate => ("Private mode", "Chế độ riêng tư"),
    TraySettings => ("Settings...", "Cài đặt..."),
    TrayQuit => ("Quit", "Thoát"),
    TrayTipActive => ("Dark Clipboard: capturing", "Dark Clipboard: đang ghi nhớ"),
    TrayTipPaused => ("Dark Clipboard: paused", "Dark Clipboard: đã tạm dừng"),
    TrayTipPausedUntil => ("Dark Clipboard: paused until", "Dark Clipboard: tạm dừng đến"),
    TrayTipPrivate => ("Dark Clipboard: private", "Dark Clipboard: riêng tư"),
    TrayTipOff => ("Dark Clipboard: capture is off", "Dark Clipboard: chưa bật ghi nhớ"),
    TrayTipFailed => (
        "Dark Clipboard: capture unavailable",
        "Dark Clipboard: không thể ghi nhớ"
    ),
    // Global shortcut (Task 203)
    ShortcutInUse => (
        "Shortcut in use by another app",
        "Phím tắt đang được ứng dụng khác dùng"
    ),
    ShortcutRegisterFailed => (
        "Shortcut could not be registered",
        "Không đăng ký được phím tắt"
    ),
    ShortcutErrEmpty => ("Shortcut is empty", "Phím tắt đang trống"),
    ShortcutErrNoKey => (
        "Shortcut needs one key besides the modifiers",
        "Phím tắt cần một phím ngoài các phím bổ trợ"
    ),
    ShortcutErrManyKeys => (
        "Shortcut can have only one key",
        "Phím tắt chỉ được có một phím"
    ),
    ShortcutErrNoModifier => (
        "Shortcut needs Ctrl, Alt or Win",
        "Phím tắt cần Ctrl, Alt hoặc Win"
    ),
    ShortcutErrDuplicate => (
        "A modifier is repeated in the shortcut",
        "Phím tắt lặp lại một phím bổ trợ"
    ),
    ShortcutErrUnknownKey => (
        "Unknown key in the shortcut",
        "Phím tắt có phím không hợp lệ"
    ),
}

/// Messages allowed to read the same in both languages.
pub const SAME_IN_BOTH: &[Msg] = &[Msg::CategoryEmail, Msg::UiAppName];

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
