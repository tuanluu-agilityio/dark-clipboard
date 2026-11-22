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
        "Delete the clipboard history? This cannot be undone.",
        "Xóa lịch sử bảng nhớ tạm? Không thể hoàn tác."
    ),
    SettingsKeepPinned => ("Keep pinned clips", "Giữ các mục đã ghim"),
    SettingsClearDo => ("Delete", "Xóa"),
    SettingsClearFailed => ("History could not be cleared", "Không xóa được lịch sử"),
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
}

/// Messages allowed to read the same in both languages.
pub const SAME_IN_BOTH: &[Msg] = &[
    Msg::CategoryEmail,
    Msg::UiAppName,
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
