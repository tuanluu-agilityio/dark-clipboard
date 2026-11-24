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
    CliCaptureOff => (
        "Capture is off until you press Start in the app. Run the app (without --headless) once with the same --data-dir and press Start.",
        "Chưa ghi lại gì cho đến khi bạn nhấn Bắt đầu trong ứng dụng. Hãy chạy ứng dụng (không có --headless) một lần với cùng --data-dir và nhấn Bắt đầu."
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
    // Short footer labels (Task 414): the full text stays in the accessible label.
    UiFooterCopyPlain => ("Plain text", "Dạng thuần"),
    UiFooterCopyAs => ("Copy as...", "Đổi dạng..."),
    UiPin => ("Pin", "Ghim"),
    UiUnpin => ("Unpin", "Bỏ ghim"),
    UiDelete => ("Delete", "Xóa"),
    UiDeleted => ("Clip deleted", "Đã xóa mục"),
    UiUndo => ("Undo", "Hoàn tác"),
    UiClose => ("Close", "Đóng"),
    // Tooltips of the icon buttons (Task 700): the name; key caps are separate strings.
    UiTipSettings => ("Settings", "Cài đặt"),
    UiTipClose => ("Close", "Đóng"),
    UiTipFavorite => ("Favorite", "Yêu thích"),
    UiKeyEsc => ("Esc", "Esc"),
    UiKeyCtrlD => ("Ctrl+D", "Ctrl+D"),
    // The popup layout (Task 701): key caps (identical in both languages) and short labels.
    UiKeyEnter => ("Enter", "Enter"),
    UiKeyShiftEnter => ("Shift+Enter", "Shift+Enter"),
    UiKeyCtrlEnter => ("Ctrl+Enter", "Ctrl+Enter"),
    UiKeySpace => ("Space", "Space"),
    UiKeyDel => ("Del", "Del"),
    UiKeyCtrlT => ("Ctrl+T", "Ctrl+T"),
    UiKeyCtrlP => ("Ctrl+P", "Ctrl+P"),
    UiKeyCtrlK => ("Ctrl+K", "Ctrl+K"),
    UiKeyCtrlH => ("Ctrl+H", "Ctrl+H"),
    UiKeyCtrlZ => ("Ctrl+Z", "Ctrl+Z"),
    UiKeyCtrlComma => ("Ctrl+,", "Ctrl+,"),
    UiPopupSearchHint => ("Search clipboard", "Tìm trong bảng nhớ tạm"),
    UiTipHistory => ("History", "Lịch sử"),
    UiHintPaste => ("Paste", "Dán"),
    UiHintPreview => ("Preview", "Xem"),
    UiTipPreview => ("Preview", "Xem trước"),
    UiMoreFilters => ("More filters", "Bộ lọc khác"),
    UiFilterLast7Days => ("Last 7 days", "7 ngày qua"),
    UiFilterApps => ("Apps", "Ứng dụng"),
    UiRowMore => ("More actions", "Thao tác khác"),
    UiMenuFavorite => ("Favorite", "Yêu thích"),
    UiMenuUnfavorite => ("Unfavorite", "Bỏ yêu thích"),
    UiMaskedRowText => ("Hidden: may contain a secret", "Đã ẩn: có thể chứa bí mật"),
    UiStatusCapturing => ("Capturing", "Đang ghi nhớ"),
    UiStatusPaused => ("Paused", "Tạm dừng"),
    UiStatusPrivate => ("Private", "Riêng tư"),
    UiStatusOff => ("Capture off", "Đã tắt ghi nhớ"),
    UiSectionPinned => ("PINNED {n}", "ĐÃ GHIM {n}"),
    UiSectionRecent => ("RECENT {n}", "GẦN ĐÂY {n}"),
    UiKeyHints => (
        "Enter copy · Shift+Enter plain · Ctrl+P pin · Ctrl+D favorite · Del delete · → details · Esc close",
        "Enter sao chép · Shift+Enter văn bản thuần · Ctrl+P ghim · Ctrl+D yêu thích · Del xóa · → chi tiết · Esc đóng"
    ),
    // Copy back and paste (Task 205)
    UiKeyHintsPaste => (
        "Enter copy · Ctrl+Enter paste · Shift+Enter plain · Ctrl+P pin · Ctrl+D favorite · Del delete · → details · Esc close",
        "Enter sao chép · Ctrl+Enter dán · Shift+Enter văn bản thuần · Ctrl+P ghim · Ctrl+D yêu thích · Del xóa · → chi tiết · Esc đóng"
    ),
    UiKeyHintsEnterPastes => (
        "Enter paste · Shift+Enter plain · Ctrl+P pin · Ctrl+D favorite · Del delete · → details · Esc close",
        "Enter dán · Shift+Enter văn bản thuần · Ctrl+P ghim · Ctrl+D yêu thích · Del xóa · → chi tiết · Esc đóng"
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
    // The preview pane's facts (Task 702).
    UiFactKind => ("Type", "Loại"),
    UiFactSource => ("Source", "Nguồn"),
    UiFactTime => ("Time", "Thời gian"),
    UiFactSize => ("Size", "Kích thước"),
    UiSizeChars => ("{n} characters", "{n} ký tự"),
    UiSizePixels => ("{w} x {h} px", "{w} x {h} px"),
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
    TrayStartCapturing => ("Start capturing...", "Bắt đầu ghi nhớ..."),
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
    // Settings window (Task 207)
    SettingsTitle => ("Dark Clipboard - Settings", "Dark Clipboard - Cài đặt"),
    SettingsOpen => ("Settings", "Cài đặt"),
    SettingsSectionGeneral => ("General", "Chung"),
    SettingsSectionPrivacy => ("Privacy", "Riêng tư"),
    SettingsSectionHistory => ("History", "Lịch sử"),
    SettingsSectionAbout => ("About", "Giới thiệu"),
    SettingsReadOnlyBanner => (
        "The settings file could not be read or is from a newer version. Defaults are shown and nothing can be saved, so your file is never overwritten.",
        "Không đọc được tệp cài đặt hoặc tệp do phiên bản mới hơn tạo ra. Đang hiện giá trị mặc định và không thể lưu, để tệp của bạn không bị ghi đè."
    ),
    SettingsSave => ("Save", "Lưu"),
    SettingsRevert => ("Revert", "Hoàn tác"),
    SettingsSaved => ("Settings saved", "Đã lưu cài đặt"),
    SettingsSaveFailed => ("Settings could not be saved", "Không lưu được cài đặt"),
    SettingsFixErrors => (
        "Fix the marked fields first",
        "Hãy sửa các mục được đánh dấu trước"
    ),
    SettingsShortcut => ("Shortcut to open the popup", "Phím tắt mở cửa sổ nhanh"),
    SettingsShortcutHint => ("Example: Ctrl+Shift+V", "Ví dụ: Ctrl+Shift+V"),
    SettingsLanguage => ("Language", "Ngôn ngữ"),
    SettingsLangEn => ("English", "English"),
    SettingsLangVi => ("Tiếng Việt", "Tiếng Việt"),
    SettingsStartAtLogin => ("Start at login", "Khởi động cùng hệ thống"),
    SettingsStartAtLoginNote => (
        "This build cannot register with the system yet; your choice is saved for later.",
        "Bản này chưa đăng ký được với hệ thống; lựa chọn của bạn được lưu để dùng sau."
    ),
    SettingsPopupPosition => ("Popup position", "Vị trí cửa sổ nhanh"),
    SettingsPosCursor => ("Near the cursor", "Gần con trỏ"),
    SettingsPosCenter => ("Center of the screen", "Giữa màn hình"),
    SettingsCloseOnFocusLoss => (
        "Close the popup when it loses focus",
        "Đóng cửa sổ nhanh khi mất tiêu điểm"
    ),
    SettingsEnterPastes => (
        "Enter also pastes into the previous app",
        "Enter cũng dán vào ứng dụng trước đó"
    ),
    SettingsCaptureMode => ("Capture", "Ghi nhớ"),
    SettingsModeNormal => ("Capturing", "Đang ghi nhớ"),
    SettingsModePrivate => ("Private mode", "Chế độ riêng tư"),
    SettingsModePaused => ("Pause until resumed", "Tạm dừng đến khi tiếp tục"),
    SettingsMarkersLocked => (
        "OS and Darkshot privacy markers: always respected",
        "Dấu riêng tư của hệ điều hành và Darkshot: luôn được tôn trọng"
    ),
    SettingsMarkersNote => (
        "Content that the system, a password manager or Darkshot marks as private is never stored. This cannot be turned off.",
        "Nội dung mà hệ thống, trình quản lý mật khẩu hoặc Darkshot đánh dấu là riêng tư sẽ không bao giờ được lưu. Không thể tắt mục này."
    ),
    SettingsSensitive => (
        "Sensitive data (keys, passwords, cards)",
        "Dữ liệu nhạy cảm (khóa, mật khẩu, thẻ)"
    ),
    SettingsSensDrop => ("Do not store", "Không lưu"),
    SettingsSensMasked => ("Store masked", "Lưu ở dạng đã che"),
    SettingsSensStore => ("Store as is", "Lưu nguyên văn"),
    SettingsSensStoreWarn => (
        "Sensitive data will be saved readable in your history.",
        "Dữ liệu nhạy cảm sẽ được lưu ở dạng đọc được trong lịch sử."
    ),
    SettingsKinds => ("Kinds of content to keep", "Loại nội dung cần lưu"),
    SettingsExcluded => ("Excluded apps", "Ứng dụng bị loại trừ"),
    SettingsExcludedNote => (
        "Copies made in these programs are never stored. Programs are matched by file name.",
        "Nội dung sao chép trong các chương trình này không bao giờ được lưu. Chương trình được nhận biết theo tên tệp."
    ),
    SettingsBuiltinBadge => ("built-in", "có sẵn"),
    SettingsRemove => ("Remove", "Gỡ"),
    SettingsAddApp => ("Add", "Thêm"),
    SettingsAppPlaceholder => (
        "Program name, e.g. keepass.exe",
        "Tên chương trình, ví dụ keepass.exe"
    ),
    SettingsPickRunning => ("Pick running app", "Chọn ứng dụng đang chạy"),
    SettingsPickNone => ("No running programs found", "Không thấy chương trình nào đang chạy"),
    SettingsPickNote => (
        "Only program file names are read, never window titles.",
        "Chỉ đọc tên tệp chương trình, không bao giờ đọc tiêu đề cửa sổ."
    ),
    SettingsConfirmRemoveBuiltin => (
        "Remove this built-in exclusion? Copies from this password manager will be stored again.",
        "Gỡ loại trừ có sẵn này? Nội dung sao chép từ trình quản lý mật khẩu này sẽ được lưu lại."
    ),
    SettingsConfirmRemoveDarkshot => (
        "Remove this built-in exclusion? Darkshot marks its own copies, so they stay excluded; this entry is only an extra safeguard.",
        "Gỡ loại trừ có sẵn này? Darkshot tự đánh dấu nội dung nó sao chép nên chúng vẫn được loại trừ; mục này chỉ là lớp bảo vệ thêm."
    ),
    SettingsErrNumber => ("Enter a whole number", "Nhập một số nguyên"),
    SettingsErrRange => (
        "The number is outside the allowed range",
        "Số nằm ngoài khoảng cho phép"
    ),
    SettingsErrNoKinds => (
        "Keep at least one kind of content",
        "Hãy giữ ít nhất một loại nội dung"
    ),
    SettingsErrAppEmpty => ("Enter a program name", "Hãy nhập tên chương trình"),
    SettingsErrAppInvalid => (
        "Use the program file name only, without a folder path",
        "Chỉ dùng tên tệp chương trình, không kèm đường dẫn thư mục"
    ),
    SettingsErrAppDuplicate => (
        "This program is already in the list",
        "Chương trình này đã có trong danh sách"
    ),
    SettingsMaxItems => (
        "Keep at most this many clips (0 = no limit, up to 1,000,000)",
        "Giữ tối đa số mục này (0 = không giới hạn, tối đa 1.000.000)"
    ),
    SettingsMaxAge => (
        "Delete clips older than this many days (0 = no limit, up to 3,650)",
        "Xóa mục cũ hơn số ngày này (0 = không giới hạn, tối đa 3.650)"
    ),
    SettingsMaxStorage => (
        "Storage limit in MB (10 - 102,400)",
        "Giới hạn dung lượng, MB (10 - 102.400)"
    ),
    SettingsRetentionNote => (
        "Pinned clips are never removed by these limits.",
        "Các mục đã ghim không bao giờ bị xóa bởi những giới hạn này."
    ),
    SettingsSaveReadOnly => (
        "Not saved: the settings file is read-only",
        "Chưa lưu: tệp cài đặt ở chế độ chỉ đọc"
    ),
    SettingsLimitsConfirm => (
        "A limit was lowered. Older clips beyond the new limit will be deleted at the next cleanup (pinned clips stay). Save anyway?",
        "Một giới hạn đã được hạ thấp. Các mục cũ vượt giới hạn mới sẽ bị xóa ở lần dọn dẹp tiếp theo (mục đã ghim được giữ lại). Vẫn lưu?"
    ),
    SettingsNotices => (
        "Copyright (C) 2026 Dark Clipboard contributors\nThis program comes with ABSOLUTELY NO WARRANTY; it is free software under GPL-3.0-only, and you are welcome to redistribute it under its conditions.\nThe full licence text is the LICENSE file in the installation folder and in the source repository; it is also at https://www.gnu.org/licenses/gpl-3.0.html",
        "Bản quyền (C) 2026 Những người đóng góp Dark Clipboard\nChương trình này KHÔNG CÓ BẤT KỲ BẢO HÀNH NÀO; đây là phần mềm tự do theo GPL-3.0-only, bạn được phép phân phối lại theo các điều kiện của giấy phép.\nToàn văn giấy phép là tệp LICENSE trong thư mục cài đặt và trong kho mã nguồn; cũng có tại https://www.gnu.org/licenses/gpl-3.0.html"
    ),
    PlatformWindows => ("Windows", "Windows"),
    PlatformMacos => ("macOS", "macOS"),
    PlatformLinux => ("Linux", "Linux"),
    SettingsStats => ("Storage", "Lưu trữ"),
    SettingsStatsClips => ("{n} clips", "{n} mục"),
    SettingsStatsPinned => ("{n} pinned", "{n} đã ghim"),
    SettingsStatsMb => ("{n} MB used", "Đã dùng {n} MB"),
    SettingsSkipped => (
        "Copies skipped since the app started",
        "Lần sao chép bị bỏ qua từ khi ứng dụng chạy"
    ),
    SettingsSkipNone => ("None", "Không có"),
    SkipDisabled => ("Capture off", "Ghi nhớ đang tắt"),
    SkipPaused => ("Paused", "Đang tạm dừng"),
    SkipPrivate => ("Private mode", "Chế độ riêng tư"),
    SkipSelfWrite => ("Made by Dark Clipboard", "Do Dark Clipboard tạo ra"),
    SkipOsConcealed => ("Marked private by the system", "Được hệ thống đánh dấu riêng tư"),
    SkipDarkshotExcluded => ("Marked by Darkshot", "Được Darkshot đánh dấu"),
    SkipTransient => ("Temporary content", "Nội dung tạm thời"),
    SkipExcludedApp => ("Excluded app", "Ứng dụng bị loại trừ"),
    SkipKindDisabled => ("Kind not kept", "Loại không được lưu"),
    SkipTooLarge => ("Too large", "Quá lớn"),
    SkipNothingReadable => ("Nothing readable", "Không có gì đọc được"),
    SkipSensitive => ("Sensitive data", "Dữ liệu nhạy cảm"),
    SkipBusy => ("Clipboard busy", "Bảng nhớ tạm bận"),
    SettingsClear => ("Clear history...", "Xóa lịch sử..."),
    SettingsClearConfirm => (
        "Delete {n} clips from the history? This cannot be undone.",
        "Xóa {n} mục khỏi lịch sử? Không thể hoàn tác."
    ),
    SettingsKeepPinned => ("Keep pinned and favorite clips", "Giữ các mục đã ghim và yêu thích"),
    SettingsClearDo => ("Delete", "Xóa"),
    SettingsClearFailed => ("History could not be cleared", "Không xóa được lịch sử"),
    SettingsStatsFavorites => ("{n} favorites", "{n} yêu thích"),
    SettingsStatsDb => ("Database {n}", "Cơ sở dữ liệu {n}"),
    SettingsStatsBlobs => ("Files {n}", "Tệp {n}"),
    SettingsCleanupHeading => ("Clean up", "Dọn dẹp"),
    SettingsOlderLabel => ("Delete clips older than (days)", "Xóa các mục cũ hơn (ngày)"),
    SettingsOlderDo => ("Delete old clips...", "Xóa các mục cũ..."),
    SettingsOlderConfirm => (
        "Delete {n} clips older than the chosen number of days? Pinned and favorite clips are kept. This cannot be undone.",
        "Xóa {n} mục cũ hơn số ngày đã chọn? Các mục đã ghim và yêu thích được giữ lại. Không thể hoàn tác."
    ),
    SettingsDaysInvalid => (
        "Enter a whole number of days from 1 to 3650",
        "Nhập số ngày nguyên từ 1 đến 3650"
    ),
    SettingsOrphansDo => ("Remove orphaned files...", "Xóa tệp mồ côi..."),
    SettingsOrphansConfirm => (
        "Remove {n} orphaned data files that no clip uses? This cannot be undone.",
        "Xóa {n} tệp dữ liệu mồ côi không mục nào dùng? Không thể hoàn tác."
    ),
    SettingsOrphansDone => (
        "{n} orphaned files removed, {b} freed",
        "Đã xóa {n} tệp mồ côi, giải phóng {b}"
    ),
    SettingsCompactDo => ("Compact database...", "Thu gọn cơ sở dữ liệu..."),
    SettingsCompactConfirm => (
        "Compact the database ({n})? It needs about that much free disk space and may take a moment.",
        "Thu gọn cơ sở dữ liệu ({n})? Cần khoảng chừng đó dung lượng đĩa trống và có thể mất một lúc."
    ),
    SettingsCompactDone => (
        "Database compacted: {n} before, {b} after",
        "Đã thu gọn cơ sở dữ liệu: trước {n}, sau {b}"
    ),
    SettingsCompactNoSpace => (
        "Not enough free disk space to compact the database",
        "Không đủ dung lượng đĩa trống để thu gọn cơ sở dữ liệu"
    ),
    SettingsCleanupFailed => ("The cleanup failed", "Dọn dẹp không thành công"),
    SettingsVersion => ("Version", "Phiên bản"),
    SettingsCapabilities => (
        "What works on this system",
        "Những gì hoạt động trên hệ thống này"
    ),
    CapMonitor => ("Clipboard monitoring", "Theo dõi bảng nhớ tạm"),
    CapReadWithoutFocus => ("Read without focus", "Đọc khi không có tiêu điểm"),
    CapWrite => ("Write to the clipboard", "Ghi vào bảng nhớ tạm"),
    CapMarkers => ("Privacy markers", "Dấu riêng tư"),
    CapSourceApp => ("Source app", "Ứng dụng nguồn"),
    CapShortcut => ("Global shortcut", "Phím tắt toàn cục"),
    CapAutoPaste => ("Auto-paste", "Tự động dán"),
    CapTray => ("Tray icon", "Biểu tượng khay"),
    CapAutostart => ("Start at login", "Khởi động cùng hệ thống"),
    CapBackend => ("Backend", "Phần phụ trợ"),
    CapNative => ("Supported", "Hỗ trợ"),
    CapPolling => ("Supported by polling", "Hỗ trợ bằng cách kiểm tra định kỳ"),
    CapNo => ("Not available", "Không có"),
    CapSourceOwner => ("Owner of the clipboard", "Chủ sở hữu bảng nhớ tạm"),
    CapSourceFront => (
        "Foreground app (approximate)",
        "Ứng dụng đang hoạt động (gần đúng)"
    ),
    CapPortal => ("Through the system portal", "Qua cổng của hệ thống"),
    CapUserBound => (
        "You assign the key in system settings",
        "Bạn gán phím trong cài đặt hệ thống"
    ),
    CapSourceNone => ("Not available", "Không có"),
    SettingsLicences => ("Licences", "Giấy phép"),
    SettingsLicenceText => (
        "Dark Clipboard is free software under the GNU General Public License, version 3 only. The direct third-party components are listed below with their licences; the full list of every dependency is in Cargo.lock.",
        "Dark Clipboard là phần mềm tự do theo Giấy phép Công cộng GNU phiên bản 3 (chỉ phiên bản 3). Các thành phần bên thứ ba được dùng trực tiếp nằm bên dưới cùng giấy phép của chúng; danh sách đầy đủ mọi thư viện nằm trong Cargo.lock."
    ),
    SettingsDataFolder => ("Data folder", "Thư mục dữ liệu"),
    SettingsOpenFolder => ("Open folder", "Mở thư mục"),
    SettingsOpenFolderFailed => ("The folder could not be opened", "Không mở được thư mục"),
    // Opt-in update check (Task 412, D-057)
    SettingsUpdatesHeading => ("Updates", "Cập nhật"),
    SettingsUpdateCheck => ("Check for updates", "Kiểm tra cập nhật"),
    SettingsUpdateAuto => ("Automatically check weekly", "Tự động kiểm tra hằng tuần"),
    SettingsUpdateNote => (
        "Off by default. A check sends one request to GitHub with only the app name and version; no clips, settings or identifiers. Nothing is downloaded or installed.",
        "Mặc định tắt. Mỗi lần kiểm tra gửi một yêu cầu đến GitHub, chỉ kèm tên và phiên bản ứng dụng; không gửi mục đã chép, cài đặt hay mã định danh. Không có gì được tải về hay cài đặt."
    ),
    SettingsUpdateChecking => ("Checking...", "Đang kiểm tra..."),
    SettingsUpdateUpToDate => ("You are up to date", "Bạn đang dùng bản mới nhất"),
    SettingsUpdateAvailable => ("New version available:", "Có phiên bản mới:"),
    SettingsUpdateFailed => ("Could not check", "Không kiểm tra được"),
    SettingsUpdateOpen => ("Open the release page", "Mở trang phát hành"),
    SettingsUpdateOpenFailed => (
        "The release page could not be opened",
        "Không mở được trang phát hành"
    ),
    SettingsStartAtLoginCustomProfile => (
        "Not available with a custom data folder: the login entry would start the default profile.",
        "Không dùng được với thư mục dữ liệu tùy chọn: mục khởi động sẽ chạy hồ sơ mặc định."
    ),
    SettingsStartAtLoginDrift => (
        "The system's real state is shown; it differs from the saved setting. Save to apply the toggle.",
        "Đang hiện trạng thái thật của hệ thống, khác với cài đặt đã lưu. Nhấn Lưu để áp dụng công tắc."
    ),
    SettingsCaptureOff => (
        "Capture is off: nothing is being saved. It starts only when you press Start.",
        "Đang tắt ghi nhớ: chưa lưu gì cả. Chỉ bắt đầu khi bạn nhấn Bắt đầu."
    ),
    SettingsShowWelcome => ("Show welcome", "Hiện màn hình chào mừng"),
    SettingsAutostartFailed => (
        "Saved, but start at login could not be changed. The toggle shows the real state.",
        "Đã lưu, nhưng không thay đổi được khởi động cùng hệ thống. Công tắc hiện trạng thái thật."
    ),
    // First-run welcome (Task 208)
    WelcomeTitle => ("Welcome to Dark Clipboard", "Chào mừng đến với Dark Clipboard"),
    WelcomeShortcut => (
        "Press {shortcut} anywhere to open your clipboard history.",
        "Nhấn {shortcut} ở bất cứ đâu để mở lịch sử bảng nhớ tạm."
    ),
    WelcomeNever => (
        "Never captured: copies from password managers, anything the system marks as concealed, and copies Darkshot excludes.",
        "Không bao giờ ghi lại: nội dung sao chép từ trình quản lý mật khẩu, nội dung hệ thống đánh dấu là ẩn, và nội dung Darkshot loại trừ."
    ),
    WelcomeData => (
        "Your history stays in this folder on your computer: {path}",
        "Lịch sử của bạn nằm trong thư mục này trên máy của bạn: {path}"
    ),
    WelcomeNoCloud => (
        "No account, no cloud, no telemetry. Nothing leaves this computer.",
        "Không tài khoản, không đám mây, không gửi dữ liệu. Không có gì rời khỏi máy này."
    ),
    WelcomeCaptureOff => (
        "Nothing is captured until you press Start.",
        "Sẽ không ghi lại gì cho đến khi bạn nhấn Bắt đầu."
    ),
    WelcomeAutostart => ("Start with Windows", "Khởi động cùng Windows"),
    WelcomeStart => ("Start", "Bắt đầu"),
    WelcomeNotNow => ("Not now", "Để sau"),
    WelcomeSaveFailed => (
        "Your choice could not be saved. Try again.",
        "Không lưu được lựa chọn của bạn. Hãy thử lại."
    ),
    WelcomeAutostartFailed => (
        "Capture is on, but start at login could not be set. You can try again in Settings.",
        "Đã bật ghi lại, nhưng không đặt được khởi động cùng hệ thống. Bạn có thể thử lại trong Cài đặt."
    ),
    // Text transforms and edited copies (Task 404)
    TfCopyAs => ("Copy as...", "Sao chép dưới dạng..."),
    TfTrim => ("Trim whitespace", "Cắt khoảng trắng"),
    TfTrimLines => ("Trim each line", "Cắt khoảng trắng từng dòng"),
    TfLf => ("Line endings: LF", "Kết thúc dòng: LF"),
    TfCrlf => ("Line endings: CRLF", "Kết thúc dòng: CRLF"),
    TfUpper => ("UPPER CASE", "CHỮ HOA"),
    TfLower => ("lower case", "chữ thường"),
    TfJsonPretty => ("JSON: pretty print", "JSON: định dạng dễ đọc"),
    TfJsonMinify => ("JSON: minify", "JSON: rút gọn"),
    TfUrlEncode => ("URL encode", "Mã hóa URL"),
    TfUrlDecode => ("URL decode", "Giải mã URL"),
    TfB64Encode => ("Base64 encode", "Mã hóa Base64"),
    TfB64Decode => ("Base64 decode", "Giải mã Base64"),
    TfHtmlToText => ("HTML to text", "HTML sang văn bản"),
    TfPreviewLabel => ("Result", "Kết quả"),
    TfPickHint => (
        "Choose a transform to see the result. Up/Down choose, Enter copy, Esc back.",
        "Chọn một phép biến đổi để xem kết quả. Lên/Xuống chọn, Enter sao chép, Esc quay lại."
    ),
    TfErrTooLarge => (
        "Too large to transform (over 1 MiB).",
        "Quá lớn để biến đổi (trên 1 MiB)."
    ),
    TfErrJson => ("This text is not valid JSON.", "Văn bản này không phải JSON hợp lệ."),
    TfErrDeep => ("This JSON is nested too deeply.", "JSON này lồng quá sâu."),
    TfErrUrl => (
        "This text is not valid percent-encoding.",
        "Văn bản này không phải mã hóa phần trăm hợp lệ."
    ),
    TfErrBase64 => ("This text is not valid Base64.", "Văn bản này không phải Base64 hợp lệ."),
    TfErrUtf8 => (
        "The decoded bytes are not text.",
        "Các byte sau khi giải mã không phải văn bản."
    ),
    TfCopiedAs => ("Copied the result", "Đã sao chép kết quả"),
    TfEdit => ("Edit...", "Sửa..."),
    TfEditHint => (
        "Editing a copy. The original clip stays unchanged.",
        "Đang sửa một bản sao. Mục gốc không thay đổi."
    ),
    TfSaveNew => ("Save as new clip", "Lưu thành mục mới"),
    TfSaved => ("Saved as a new clip", "Đã lưu thành mục mới"),
    TfNotSavedSensitive => (
        "Not saved: it looks like a secret.",
        "Không lưu: nội dung có vẻ là thông tin bí mật."
    ),
    TfNotSavedKind => (
        "Not saved: this kind of clip is turned off in Settings.",
        "Không lưu: loại nội dung này đã tắt trong Cài đặt."
    ),
    TfNotSavedEmpty => ("There is nothing to save.", "Không có gì để lưu."),
    TfNotSavedLarge => (
        "Too large to save as a clip.",
        "Quá lớn để lưu thành mục."
    ),
    TfEditTooLarge => (
        "Too large to edit here (over 256 KiB).",
        "Quá lớn để sửa tại đây (trên 256 KiB)."
    ),
    TfAlreadyExists => (
        "Already in history",
        "Đã có trong lịch sử"
    ),
    TfEmptyResult => (
        "The result is empty, so there is nothing to copy.",
        "Kết quả trống nên không có gì để sao chép."
    ),
    TfNotSavedPrivate => (
        "Not saved: private mode is on.",
        "Không lưu: chế độ riêng tư đang bật."
    ),
    // Organisation: collections, tags, notes, favorites (Task 403)
    OrgFavorites => ("Favorites", "Yêu thích"),
    OrgFavoriteBadge => ("Favorite", "Yêu thích"),
    OrgGroupCollections => ("Collections", "Bộ sưu tập"),
    OrgGroupTags => ("Tags", "Thẻ"),
    OrgNewCollection => ("New collection", "Bộ sưu tập mới"),
    OrgRenameCollection => ("Rename collection", "Đổi tên bộ sưu tập"),
    OrgDeleteCollection => ("Delete collection", "Xóa bộ sưu tập"),
    OrgCollectionName => ("Collection name", "Tên bộ sưu tập"),
    OrgNameEmpty => ("Enter a name.", "Hãy nhập tên."),
    OrgNameTooLong => (
        "The name is too long (80 characters at most).",
        "Tên quá dài (tối đa 80 ký tự)."
    ),
    OrgNameExists => (
        "A collection with this name already exists.",
        "Đã có bộ sưu tập với tên này."
    ),
    OrgConfirmDeleteCollection => (
        "Delete the collection \"{name}\"? Its {n} clips stay in your history.",
        "Xóa bộ sưu tập \"{name}\"? {n} mục trong đó vẫn còn trong lịch sử."
    ),
    OrgCollectionCreated => ("Collection created", "Đã tạo bộ sưu tập"),
    OrgCollectionRenamed => ("Collection renamed", "Đã đổi tên bộ sưu tập"),
    OrgCollectionDeleted => ("Collection deleted", "Đã xóa bộ sưu tập"),
    OrgAddFavorite => ("Add to Favorites", "Thêm vào Yêu thích"),
    OrgRemoveFavorite => ("Remove from Favorites", "Bỏ khỏi Yêu thích"),
    OrgNote => ("Note", "Ghi chú"),
    OrgNoNote => ("No note", "Chưa có ghi chú"),
    OrgEditNote => ("Edit note...", "Sửa ghi chú..."),
    OrgNoteTooLong => (
        "The note is too long (4,000 characters at most).",
        "Ghi chú quá dài (tối đa 4.000 ký tự)."
    ),
    OrgNoteHint => (
        "Ctrl+Enter save · Esc cancel",
        "Ctrl+Enter lưu · Esc hủy"
    ),
    OrgSave => ("Save", "Lưu"),
    OrgApply => ("Apply", "Áp dụng"),
    OrgSaved => ("Saved", "Đã lưu"),
    OrgTagsEdit => ("Edit tags...", "Sửa thẻ..."),
    OrgAddTag => ("Add tag", "Thêm thẻ"),
    OrgTagPlaceholder => ("Type a tag and press Enter", "Nhập thẻ rồi nhấn Enter"),
    OrgTagInvalid => (
        "A tag may use letters, digits, - and _ only.",
        "Thẻ chỉ gồm chữ, số, - và _."
    ),
    OrgTagTooLong => (
        "A tag has 40 characters at most.",
        "Thẻ tối đa 40 ký tự."
    ),
    OrgTagsFull => (
        "A clip can have 20 tags at most.",
        "Mỗi mục có tối đa 20 thẻ."
    ),
    OrgTagsHint => (
        "Enter add · Backspace on empty removes the last tag · Esc close",
        "Enter thêm · Backspace khi trống bỏ thẻ cuối · Esc đóng"
    ),
    OrgRemoveTag => ("Remove tag", "Bỏ thẻ"),
    OrgNoTags => ("No tags", "Chưa có thẻ"),
    OrgAddToCollection => ("Add to collection...", "Thêm vào bộ sưu tập..."),
    OrgRemoveFromCollection => ("Remove from collection...", "Bỏ khỏi bộ sưu tập..."),
    OrgNoCollections => (
        "No collections yet. Create one with Ctrl+Shift+N.",
        "Chưa có bộ sưu tập. Tạo bằng Ctrl+Shift+N."
    ),
    OrgPickHint => (
        "Up/Down choose · Enter confirm · Esc cancel",
        "Lên/Xuống chọn · Enter xác nhận · Esc hủy"
    ),
    OrgConfirmAddToCollection => (
        "Add {n} clips to \"{name}\"?",
        "Thêm {n} mục vào \"{name}\"?"
    ),
    OrgConfirmRemoveFromCollection => (
        "Remove the {n} marked clips from \"{name}\"? Clips not in it are unchanged.",
        "Bỏ {n} mục đã chọn khỏi \"{name}\"? Mục không thuộc đó sẽ không đổi."
    ),
    OrgConfirmAddTag => (
        "Add the tag \"{name}\" to {n} clips?",
        "Thêm thẻ \"{name}\" vào {n} mục?"
    ),
    OrgConfirmFavorite => (
        "Add {n} clips to Favorites?",
        "Thêm {n} mục vào Yêu thích?"
    ),
    OrgConfirmUnfavorite => (
        "Remove {n} clips from Favorites?",
        "Bỏ {n} mục khỏi Yêu thích?"
    ),
    OrgSkippedMasked => (
        "Masked clips skipped: {n}",
        "Mục bị che đã bỏ qua: {n}"
    ),
    OrgSkippedFull => (
        "Clips already at 20 tags, skipped: {n}",
        "Mục đã đủ 20 thẻ, đã bỏ qua: {n}"
    ),
    OrgNothingToDo => (
        "Nothing to change. Masked clips cannot be favorited, tagged or added to a collection.",
        "Không có gì để đổi. Mục bị che không thể yêu thích, gắn thẻ hay thêm vào bộ sưu tập."
    ),
    OrgDone => ("{n} clips updated", "Đã cập nhật {n} mục"),
    OrgDoneInexact => ("Done", "Xong"),
    OrgKeyHintsConfirm => ("Enter apply · Esc cancel", "Enter áp dụng · Esc hủy"),
    OrgKeyHintsHistory => (
        "Ctrl+D favorite · Ctrl+N note · Ctrl+G tags · Ctrl+L add to collection · Ctrl+Shift+L remove · Ctrl+Shift+N new collection · Ctrl+Shift+E export",
        "Ctrl+D yêu thích · Ctrl+N ghi chú · Ctrl+G thẻ · Ctrl+L thêm vào bộ sưu tập · Ctrl+Shift+L bỏ khỏi · Ctrl+Shift+N bộ sưu tập mới · Ctrl+Shift+E xuất"
    ),
    // Backup, restore, export and import (Task 405)
    ExportSelectedName => ("Selected clips", "Mục đã chọn"),
    ImportCollectionName => ("Imported clips", "Mục đã nhập"),
    SettingsTransferHeading => ("Backup and transfer", "Sao lưu và chuyển dữ liệu"),
    SettingsBackupDo => ("Back up now...", "Sao lưu ngay..."),
    SettingsRestoreDo => ("Restore from backup...", "Khôi phục từ bản sao lưu..."),
    SettingsImportDo => ("Import clips...", "Nhập mục..."),
    SettingsTransferNote => (
        "Backups and exports leave out masked and sensitive clips. A backup keeps every other clip, including clips that apps asked not to sync, in a plain zip file that is not encrypted: saving it in a synced folder (OneDrive, Dropbox) uploads your history. Exports also leave out those no-sync clips. Imported clips are checked like copied ones: secrets follow your sensitive-content setting. Export a collection or marked clips from the history window with Ctrl+Shift+E.",
        "Bản sao lưu và tệp xuất bỏ qua mục bị che và mục nhạy cảm. Bản sao lưu giữ mọi mục khác, kể cả mục mà ứng dụng yêu cầu không đồng bộ, trong một tệp zip thường không mã hóa: lưu nó trong thư mục được đồng bộ (OneDrive, Dropbox) sẽ tải lịch sử của bạn lên mạng. Tệp xuất còn bỏ qua các mục không đồng bộ đó. Mục được nhập được kiểm tra như mục sao chép: bí mật theo cài đặt nội dung nhạy cảm của bạn. Xuất một bộ sưu tập hoặc các mục đã chọn trong cửa sổ lịch sử bằng Ctrl+Shift+E."
    ),
    SettingsRestoreConfirm => (
        "Replace your whole history with this backup? Your current history is kept as a safety copy until Dark Clipboard starts again.",
        "Thay toàn bộ lịch sử bằng bản sao lưu này? Lịch sử hiện tại được giữ làm bản an toàn đến khi Dark Clipboard khởi động lại."
    ),
    SettingsRestoreYes => ("Restore", "Khôi phục"),
    TransferWorking => ("Working...", "Đang xử lý..."),
    TransferBackupDone => (
        "Backup saved: {n} clips. It is not encrypted: keep it out of synced folders.",
        "Đã lưu bản sao lưu: {n} mục. Tệp không được mã hóa: đừng để trong thư mục được đồng bộ."
    ),
    TransferRestoreDone => ("History restored: {n} clips", "Đã khôi phục lịch sử: {n} mục"),
    TransferImportDone => (
        "Imported {n} new clips; {m} were already in your history",
        "Đã nhập {n} mục mới; {m} mục đã có trong lịch sử"
    ),
    TransferImportSkipped => (
        "Not imported: {s} secrets, {k} turned-off kinds or file lists, {l} too large, {d} damaged",
        "Không nhập: {s} bí mật, {k} loại đã tắt hoặc danh sách tệp, {l} quá lớn, {d} bị hỏng"
    ),
    TransferExportDone => ("Exported {n} clips", "Đã xuất {n} mục"),
    TransferExportExcluded => (
        "Masked, sensitive or no-export clips left out: {n}",
        "Mục bị che, nhạy cảm hoặc không được xuất đã bỏ qua: {n}"
    ),
    TransferNewer => (
        "This file was made by a newer version of Dark Clipboard. Update the app to open it. Nothing was changed.",
        "Tệp này được tạo bởi phiên bản Dark Clipboard mới hơn. Hãy cập nhật ứng dụng để mở. Không có gì thay đổi."
    ),
    TransferInvalid => (
        "This is not a valid Dark Clipboard file, or it is damaged. Nothing was changed.",
        "Đây không phải tệp Dark Clipboard hợp lệ hoặc tệp đã hỏng. Không có gì thay đổi."
    ),
    TransferTooLarge => (
        "This file is too large to open safely. Nothing was changed.",
        "Tệp này quá lớn để mở an toàn. Không có gì thay đổi."
    ),
    TransferTooLargeToWrite => (
        "This is more than Dark Clipboard could open again from one file. Nothing was written; export fewer clips.",
        "Dữ liệu này nhiều hơn mức Dark Clipboard có thể mở lại từ một tệp. Không có gì được ghi; hãy xuất ít mục hơn."
    ),
    TransferNoSpace => (
        "Not enough free disk space to restore this backup safely. Nothing was changed.",
        "Không đủ dung lượng đĩa trống để khôi phục an toàn bản sao lưu này. Không có gì thay đổi."
    ),
    TransferPrivate => (
        "Private mode is on: nothing is imported.",
        "Chế độ riêng tư đang bật: không nhập gì."
    ),
    TransferFailed => (
        "The file could not be read or written. Nothing was changed.",
        "Không thể đọc hoặc ghi tệp. Không có gì thay đổi."
    ),
    TransferNoDialog => (
        "Choosing a file is not available on this platform yet.",
        "Chưa thể chọn tệp trên nền tảng này."
    ),
    TransferDialogBackup => ("Save a backup", "Lưu bản sao lưu"),
    TransferDialogRestore => ("Choose a backup to restore", "Chọn bản sao lưu để khôi phục"),
    TransferDialogImport => ("Choose a file to import", "Chọn tệp để nhập"),
    TransferDialogExport => ("Export clips", "Xuất mục"),
    TransferFilterBackup => ("Dark Clipboard backup", "Bản sao lưu Dark Clipboard"),
    TransferFilterExport => ("Dark Clipboard export", "Tệp xuất Dark Clipboard"),
    OrgCollectionKeys => (
        "Ctrl+Shift+R rename the shown collection · Ctrl+Shift+Del delete it",
        "Ctrl+Shift+R đổi tên bộ sưu tập đang xem · Ctrl+Shift+Del xóa"
    ),
    // Start-up problems shown in a message window (Task 413). `{path}` is the data folder.
    StartDbDamaged => (
        "Dark Clipboard could not open its history: the database file is damaged.\n\nFolder: {path}\n\nChoose Yes to move the damaged history aside and start with an empty history. Nothing is deleted, and you can then restore a backup in Settings. The moved files (history.db.damaged-<time>, blobs.damaged-<time> and their companions in that folder) keep all your old clips, including ones that would have expired; Clear history does not remove them. Delete them yourself when you no longer need them.\n\nChoose No to quit.",
        "Dark Clipboard không mở được lịch sử: tệp cơ sở dữ liệu bị hỏng.\n\nThư mục: {path}\n\nChọn Có để chuyển lịch sử bị hỏng sang một bên và bắt đầu với lịch sử trống. Không có gì bị xóa, và sau đó bạn có thể khôi phục bản sao lưu trong Cài đặt. Các tệp được chuyển (history.db.damaged-<thời điểm>, blobs.damaged-<thời điểm> và các tệp đi kèm trong thư mục đó) giữ mọi mục cũ, kể cả mục lẽ ra đã hết hạn; Xóa lịch sử không xóa chúng. Hãy tự xóa chúng khi không cần nữa.\n\nChọn Không để thoát."
    ),
    StartDbTooNew => (
        "This history was created by a newer version of Dark Clipboard, so this version cannot open it. Install the newer version. Nothing was changed.\n\nFolder: {path}",
        "Lịch sử này được tạo bởi phiên bản Dark Clipboard mới hơn nên phiên bản này không mở được. Hãy cài phiên bản mới hơn. Không có gì thay đổi.\n\nThư mục: {path}"
    ),
    StartDbFailed => (
        "Dark Clipboard could not open its history. Check that the folder exists and that you can write to it.\n\nFolder: {path}",
        "Dark Clipboard không mở được lịch sử. Hãy kiểm tra thư mục còn tồn tại và bạn có quyền ghi vào đó.\n\nThư mục: {path}"
    ),
    CliDbDamaged => (
        "Dark Clipboard could not open its history: the database file is damaged. Start the app without --headless to move it aside and start empty. Nothing was changed.\n\nFolder: {path}",
        "Dark Clipboard không mở được lịch sử: tệp cơ sở dữ liệu bị hỏng. Hãy mở ứng dụng không có --headless để chuyển nó sang một bên và bắt đầu với lịch sử trống. Không có gì thay đổi.\n\nThư mục: {path}"
    ),
    StartMoveAsideFailed => (
        "The damaged history could not be moved aside. Nothing was deleted. Close any program that uses the folder and start Dark Clipboard again.\n\nFolder: {path}",
        "Không chuyển được lịch sử bị hỏng sang một bên. Không có gì bị xóa. Hãy đóng các chương trình đang dùng thư mục rồi mở lại Dark Clipboard.\n\nThư mục: {path}"
    ),
    StartPipeInUse => (
        "Dark Clipboard could not start: another program is using its connection.",
        "Dark Clipboard không khởi động được: một chương trình khác đang dùng kết nối của nó."
    ),
}

/// Messages allowed to read the same in both languages.
pub const SAME_IN_BOTH: &[Msg] = &[
    Msg::CategoryEmail,
    Msg::UiAppName,
    Msg::UiKeyEsc,
    Msg::UiKeyCtrlD,
    Msg::UiKeyEnter,
    Msg::UiKeyShiftEnter,
    Msg::UiKeyCtrlEnter,
    Msg::UiKeySpace,
    Msg::UiKeyDel,
    Msg::UiKeyCtrlT,
    Msg::UiKeyCtrlP,
    Msg::UiKeyCtrlK,
    Msg::UiKeyCtrlH,
    Msg::UiKeyCtrlZ,
    Msg::UiKeyCtrlComma,
    Msg::UiSizePixels,
    Msg::SettingsLangEn,
    Msg::SettingsLangVi,
    Msg::PlatformWindows,
    Msg::PlatformMacos,
    Msg::PlatformLinux,
];

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
