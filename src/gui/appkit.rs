//! AppKit windowing bindings for the GUI examples — `NSApplication`,
//! `NSWindow`, `NSView`, `NSEvent` and `NSDate` declared by hand against the
//! Objective-C ABI, exactly the way the library declares `MTLDevice` and
//! friends. The `windows`-side of the house style does not apply here: this
//! crate is macOS-only, so the AppKit module is the windowing half of every
//! GUI example (NSApp + layer-hosted `CAMetalLayer` swapchain).

#![allow(non_snake_case)]
#![allow(dead_code)]

use std::ffi::CStr;
use std::sync::LazyLock;

use metal_cpp_rs::foundation::NSString;
use metal_cpp_rs::objc_abi::{
    CFTimeInterval, CGFloat, Class, NSInteger, NSUInteger, Obj, ObjBOOL, Raw, Ref, Sel, Strong,
    bool_arg, send, send_bool, send_stret, send1, send2, send3, send4, send9, send10,
};
use metal_cpp_rs::quartz_core::CGSize;

/// `NSPoint` from `CoreGraphics/CGGeometry.h` (16 bytes — returned in
/// registers, no `_stret` needed).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct NSPoint {
    pub x: CGFloat,
    pub y: CGFloat,
}

/// `NSRect` from `CoreGraphics/CGGeometry.h` (32 bytes — `stret` on x86_64).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct NSRect {
    pub origin: NSPoint,
    pub size: CGSize,
}

// ---------------------------------------------------------------------------
// Force-link AppKit (a class symbol is the only stable anchor).
// ---------------------------------------------------------------------------

#[cfg_attr(target_vendor = "apple", link(name = "AppKit", kind = "framework"))]
unsafe extern "C" {
    #[link_name = "OBJC_CLASS_$_NSApplication"]
    static _NS_APPLICATION_CLASS: u8;
}

/// Keeps the `OBJC_CLASS_$_NSApplication` reference alive so the linker emits
/// `LC_LOAD_DYLIB` for AppKit. Call once before any AppKit class lookup.
pub fn appkit_force_link() -> usize {
    std::ptr::addr_of!(_NS_APPLICATION_CLASS) as usize
}

// ---------------------------------------------------------------------------
// NSEvent / window constants.
// ---------------------------------------------------------------------------

/// `NSEventTypeLeftMouseDown`.
pub const EVENT_LEFT_MOUSE_DOWN: NSUInteger = 1;
/// `NSEventTypeLeftMouseUp`.
pub const EVENT_LEFT_MOUSE_UP: NSUInteger = 2;
/// `NSEventTypeRightMouseDown`.
pub const EVENT_RIGHT_MOUSE_DOWN: NSUInteger = 3;
/// `NSEventTypeRightMouseUp`.
pub const EVENT_RIGHT_MOUSE_UP: NSUInteger = 4;
/// `NSEventTypeMouseMoved`.
pub const EVENT_MOUSE_MOVED: NSUInteger = 5;
/// `NSEventTypeRightMouseDragged`.
pub const EVENT_RIGHT_MOUSE_DRAGGED: NSUInteger = 6;
/// `NSEventTypeLeftMouseDragged`.
pub const EVENT_LEFT_MOUSE_DRAGGED: NSUInteger = 7;
/// `NSEventTypeKeyDown`.
pub const EVENT_KEY_DOWN: NSUInteger = 10;
/// `NSEventTypeKeyUp`.
pub const EVENT_KEY_UP: NSUInteger = 11;
/// `NSEventTypeFlagsChanged`.
pub const EVENT_FLAGS_CHANGED: NSUInteger = 12;

/// `NSWindowStyleMaskTitled | Closable | Miniaturizable | Resizable`.
pub const WINDOW_STYLE_DEFAULT: NSUInteger = 1 | 2 | 4 | 8;
/// `NSBackingStoreBuffered`.
pub const WINDOW_BACKING_BUFFERED: NSUInteger = 2;
/// `NSApplicationActivationPolicyRegular`.
pub const ACTIVATION_POLICY_REGULAR: i64 = 0;
/// `NSEventModifierFlagShift`.
pub const MODIFIER_FLAG_SHIFT: NSUInteger = 1 << 17;
/// `NSEventModifierFlagControl`.
pub const MODIFIER_FLAG_CONTROL: NSUInteger = 1 << 18;
/// `NSEventModifierFlagOption`.
pub const MODIFIER_FLAG_OPTION: NSUInteger = 1 << 19;
/// `NSEventModifierFlagCommand`.
pub const MODIFIER_FLAG_COMMAND: NSUInteger = 1 << 20;

// ---------------------------------------------------------------------------
// Lazy selector / class caches — the same pattern as the library's
// `sel!` / `class!` macros (which are pub(crate), so the examples replicate
// them here against the public ABI surface).
// ---------------------------------------------------------------------------

macro_rules! ns_sel {
    ($name:literal) => {{
        static SEL: LazyLock<Sel> = LazyLock::new(|| {
            let c = CStr::from_bytes_with_nul(concat!($name, "\0").as_bytes())
                .expect("selector literals must not contain interior NUL bytes");
            unsafe { Sel::from_cstr(c) }
        });
        *LazyLock::force(&SEL)
    }};
}

macro_rules! ns_class {
    ($(#[$m:meta])* $vis:vis struct $name:ident : $cls:literal;) => {
        $(#[$m])*
        #[repr(transparent)]
        #[derive(Clone, Copy)]
        $vis struct $name(Raw);

        impl Obj for $name {
            fn from_raw(ptr: Raw) -> Self {
                $name(ptr)
            }
            fn as_raw(&self) -> Raw {
                self.0
            }
            unsafe fn class_ptr() -> Class {
                static CLS: LazyLock<Class> = LazyLock::new(|| {
                    let c = CStr::from_bytes_with_nul(concat!($cls, "\0").as_bytes())
                        .expect("class-name literals must not contain interior NUL bytes");
                    let cls = unsafe { Class::from_cstr(c) };
                    if cls.is_nil() {
                        panic!("Objective-C class '{}' not found — AppKit is required", $cls);
                    }
                    cls
                });
                *LazyLock::force(&CLS)
            }
        }

        impl $name {
            /// The nil object.
            pub fn nil() -> Self {
                $name(std::ptr::null_mut())
            }
            pub fn is_nil(&self) -> bool {
                self.0.is_null()
            }
            /// `+[Cls alloc]` (+1, uninitialized).
            pub unsafe fn alloc() -> Strong<Self> {
                Strong::from_transfer(metal_cpp_rs::objc_abi::alloc_raw($name::class_ptr()))
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({:p})", stringify!($name), self.0)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Classes.
// ---------------------------------------------------------------------------

ns_class!(
    /// `NSApplication` — the shared app object and its event loop plumbing.
    pub struct NSApplication : "NSApplication";
);

impl NSApplication {
    /// `+[NSApplication sharedApplication]` (autoreleased +0 → retained).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn sharedApplication() -> Strong<NSApplication> {
        Strong::from_retain(send::<Raw>(
            NSApplication::class_ptr().ptr(),
            ns_sel!("sharedApplication"),
        ))
    }

    /// `-setActivationPolicy:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setActivationPolicy(&self, policy: i64) {
        send1::<NSInteger, ()>(self.as_raw(), ns_sel!("setActivationPolicy:"), policy);
    }

    /// `-activateIgnoringOtherApps:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn activateIgnoringOtherApps(&self, flag: bool) {
        send1::<ObjBOOL, ()>(
            self.as_raw(),
            ns_sel!("activateIgnoringOtherApps:"),
            bool_arg(flag),
        );
    }

    /// `-nextEventMatchingMask:untilDate:inMode:dequeue:`
    /// (autoreleased +0 → retained; nil when nothing pending).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn nextEventMatchingMask_untilDate_inMode_dequeue(
        &self,
        mask: NSUInteger,
        until: Raw,
        mode: Raw,
        dequeue: bool,
    ) -> Strong<NSEvent> {
        let ev = send4::<NSUInteger, Raw, Raw, ObjBOOL, Raw>(
            self.as_raw(),
            ns_sel!("nextEventMatchingMask:untilDate:inMode:dequeue:"),
            mask,
            until,
            mode,
            bool_arg(dequeue),
        );
        Strong::from_retain(ev)
    }

    /// `-sendEvent:` — dispatch an event to the window it belongs to.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn sendEvent(&self, event: Ref<NSEvent>) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("sendEvent:"), event.as_raw());
    }

    /// `-postEvent:atStart:` — put an event on the application queue, where
    /// the next `nextEventMatchingMask:...` returns it.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn postEvent_atStart(&self, event: Ref<NSEvent>, at_start: bool) {
        send2::<Raw, ObjBOOL, ()>(
            self.as_raw(),
            ns_sel!("postEvent:atStart:"),
            event.as_raw(),
            bool_arg(at_start),
        );
    }

    /// `-isActive` — is this application the frontmost one?
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn isActive(&self) -> bool {
        send_bool(self.as_raw(), ns_sel!("isActive"))
    }

    /// `-finishLaunching` — completes `NSApplication` start-up (connects to
    /// the window server, posts `ApplicationDidFinishLaunching`). `run` is
    /// never called because the examples pump events themselves, so this
    /// has to be sent explicitly, before any activation request.
    ///
    /// # Safety
    /// Must be called on the main thread. Calls into the Objective-C runtime.
    pub unsafe fn finishLaunching(&self) {
        send::<()>(self.as_raw(), ns_sel!("finishLaunching"));
    }

    /// `-activate` (macOS 14+) or the older `-activateIgnoringOtherApps:YES`.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn activate(&self) {
        let sel = ns_sel!("activate");
        if self.respondsToSelector(sel) {
            send::<()>(self.as_raw(), sel);
        } else {
            send1::<ObjBOOL, ()>(
                self.as_raw(),
                ns_sel!("activateIgnoringOtherApps:"),
                bool_arg(true),
            );
        }
    }

    /// `-setMainMenu:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setMainMenu(&self, menu: Ref<NSMenu>) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("setMainMenu:"), menu.as_raw());
    }

    /// `-terminate:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn terminate(&self, sender: Raw) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("terminate:"), sender);
    }
}

ns_class!(
    /// `NSWindow` — a titled, closable, resizable window.
    pub struct NSWindow : "NSWindow";
);

impl NSWindow {
    /// `-initWithContentRect:styleMask:backing:defer:` — consumes the
    /// alloc's +1 and returns the initialized window. `init…` hands the
    /// same reference back, so the returned `Strong` owns it and the
    /// argument must not be released a second time.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn init_with_content_rect(
        window: Strong<NSWindow>,
        content_rect: NSRect,
        style: NSUInteger,
        backing: NSUInteger,
        defer: bool,
    ) -> Strong<NSWindow> {
        let raw = window.into_raw();
        let initialized = send4::<NSRect, NSUInteger, NSUInteger, ObjBOOL, Raw>(
            raw,
            ns_sel!("initWithContentRect:styleMask:backing:defer:"),
            content_rect,
            style,
            backing,
            bool_arg(defer),
        );
        Strong::from_transfer(initialized)
    }

    /// `-setTitle:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setTitle(&self, title: Ref<NSString>) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("setTitle:"), title.as_raw());
    }

    /// `-makeKeyAndOrderFront:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn makeKeyAndOrderFront(&self, sender: Raw) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("makeKeyAndOrderFront:"), sender);
    }

    /// `-cascadeTopLeftFromPoint:` (16-byte `NSPoint` return — registers).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn cascadeTopLeftFromPoint(&self, point: NSPoint) -> NSPoint {
        send1::<NSPoint, NSPoint>(self.as_raw(), ns_sel!("cascadeTopLeftFromPoint:"), point)
    }

    /// `-setAcceptsMouseMovedEvents:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setAcceptsMouseMovedEvents(&self, flag: bool) {
        send1::<ObjBOOL, ()>(
            self.as_raw(),
            ns_sel!("setAcceptsMouseMovedEvents:"),
            bool_arg(flag),
        );
    }

    /// `-isVisible`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn isVisible(&self) -> bool {
        send_bool(self.as_raw(), ns_sel!("isVisible"))
    }

    /// `-isMiniaturized`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn isMiniaturized(&self) -> bool {
        send_bool(self.as_raw(), ns_sel!("isMiniaturized"))
    }

    /// `-contentView`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn contentView(&self) -> Ref<NSView> {
        Ref::from_raw(send::<Raw>(self.as_raw(), ns_sel!("contentView")))
    }

    /// `-backingScaleFactor` (`CGFloat` = `double`).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn backingScaleFactor(&self) -> CGFloat {
        send::<CGFloat>(self.as_raw(), ns_sel!("backingScaleFactor"))
    }

    /// `-windowNumber` — the window-server identifier.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn windowNumber(&self) -> NSInteger {
        send::<NSInteger>(self.as_raw(), ns_sel!("windowNumber"))
    }

    /// `-isKeyWindow` — does this window receive keyboard events?
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn isKeyWindow(&self) -> bool {
        send_bool(self.as_raw(), ns_sel!("isKeyWindow"))
    }

    /// `-makeKeyWindow`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn makeKeyWindow(&self) {
        send::<()>(self.as_raw(), ns_sel!("makeKeyWindow"));
    }

    /// `-convertPointToBacking:` — points → pixels.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn convertPointToBacking(&self, point: NSPoint) -> NSPoint {
        send1::<NSPoint, NSPoint>(self.as_raw(), ns_sel!("convertPointToBacking:"), point)
    }

    /// `-contentRect` (for frame → content mapping).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn contentRect(&self) -> NSRect {
        send_stret::<NSRect>(self.as_raw(), ns_sel!("contentRect"))
    }

    /// `-setFrame:display:` — move/resize the window.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setFrame_display(&self, frame: NSRect, display: bool) {
        send2::<NSRect, ObjBOOL, ()>(
            self.as_raw(),
            ns_sel!("setFrame:display:"),
            frame,
            bool_arg(display),
        );
    }

    /// `-performClose:` — the close button's action.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn performClose(&self, sender: Raw) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("performClose:"), sender);
    }

    /// `-setReleasedWhenClosed:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setReleasedWhenClosed(&self, flag: bool) {
        send1::<ObjBOOL, ()>(
            self.as_raw(),
            ns_sel!("setReleasedWhenClosed:"),
            bool_arg(flag),
        );
    }

    /// `-isReleasedWhenClosed`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn isReleasedWhenClosed(&self) -> bool {
        send_bool(self.as_raw(), ns_sel!("isReleasedWhenClosed"))
    }

    /// `-setFrameTopLeftPoint:` — position the window frame (points, screen
    /// coordinates, origin bottom-left).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setFrameTopLeftPoint(&self, point: NSPoint) {
        send1::<NSPoint, ()>(self.as_raw(), ns_sel!("setFrameTopLeftPoint:"), point);
    }
}

/// `+[NSEvent modifierFlags]` — the modifier keys held right now, whether
/// or not a flags-changed event has been delivered to this window.
///
/// # Safety
/// Calls into the Objective-C runtime.
pub unsafe fn current_modifier_flags() -> NSUInteger {
    send::<NSUInteger>(NSEvent::class_ptr().ptr(), ns_sel!("modifierFlags"))
}

ns_class!(
    /// `NSMenu` — used for the minimal menu bar every example installs.
    pub struct NSMenu : "NSMenu";
);

ns_class!(
    /// `NSMenuItem` — one entry of an `NSMenu`.
    pub struct NSMenuItem : "NSMenuItem";
);

impl NSMenu {
    /// `-initWithTitle:` — consumes the alloc's +1 (the returned `Strong`
    /// owns it; `init…` hands the same reference back, so the argument must
    /// not be released a second time).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn initWithTitle(menu: Strong<NSMenu>, title: Ref<NSString>) -> Strong<NSMenu> {
        let raw = menu.into_raw();
        Strong::from_transfer(send1::<Raw, Raw>(
            raw,
            ns_sel!("initWithTitle:"),
            title.as_raw(),
        ))
    }

    /// `-addItemWithTitle:action:keyEquivalent:` (+0 autoreleased →
    /// retained). A null `action` makes a plain title (menu-bar root); a
    /// `key` of `""` means no shortcut.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn addItemWithTitle_action_keyEquivalent(
        &self,
        title: Ref<NSString>,
        action: Sel,
        key: &str,
    ) -> Strong<NSMenuItem> {
        let key = NSString::from_str(key);
        Strong::from_retain(send3::<Raw, Sel, Raw, Raw>(
            self.as_raw(),
            ns_sel!("addItemWithTitle:action:keyEquivalent:"),
            title.as_raw(),
            action,
            key.as_ref().as_raw(),
        ))
    }

    /// `-setAutoenablesItems:` — `NO` keeps every item enabled without
    /// asking the responder chain to validate it.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setAutoenablesItems(&self, flag: bool) {
        send1::<ObjBOOL, ()>(
            self.as_raw(),
            ns_sel!("setAutoenablesItems:"),
            bool_arg(flag),
        );
    }
}

impl NSMenuItem {
    /// `-setSubmenu:` — turns the item into a menu-bar title.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setSubmenu(&self, menu: Ref<NSMenu>) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("setSubmenu:"), menu.as_raw());
    }

    /// `-setKeyEquivalentModifierMask:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setKeyEquivalentModifierMask(&self, mask: NSUInteger) {
        send1::<NSUInteger, ()>(
            self.as_raw(),
            ns_sel!("setKeyEquivalentModifierMask:"),
            mask,
        );
    }
}

/// Install a minimal menu bar: an application menu with `Quit <title> ⌘Q`
/// and a Window menu with `Close ⌘W`. Without it an unbundled executable
/// shows an empty menu bar while it is frontmost and `⌘Q` does nothing.
///
/// The shortcut is dispatched by AppKit, so key events that carry the
/// Command modifier must reach `-sendEvent:` (see `App::handle_event`).
///
/// # Safety
/// Calls into the Objective-C runtime; main thread only.
pub unsafe fn install_main_menu(app: &NSApplication, title: &str) {
    let bar = NSMenu::initWithTitle(NSMenu::alloc(), NSString::from_str("").as_ref());
    bar.setAutoenablesItems(false);

    let app_menu = NSMenu::initWithTitle(NSMenu::alloc(), NSString::from_str("").as_ref());
    let quit = app_menu.addItemWithTitle_action_keyEquivalent(
        NSString::from_str(&format!("Quit {title}")).as_ref(),
        ns_sel!("terminate:"),
        "q",
    );
    quit.setKeyEquivalentModifierMask(MODIFIER_FLAG_COMMAND);

    let window_menu = NSMenu::initWithTitle(NSMenu::alloc(), NSString::from_str("Window").as_ref());
    let close = window_menu.addItemWithTitle_action_keyEquivalent(
        NSString::from_str("Close Window").as_ref(),
        ns_sel!("performClose:"),
        "w",
    );
    close.setKeyEquivalentModifierMask(MODIFIER_FLAG_COMMAND);

    for (name, submenu) in [(title, app_menu), ("Window", window_menu)] {
        let root = bar.addItemWithTitle_action_keyEquivalent(
            NSString::from_str(name).as_ref(),
            Sel(std::ptr::null()),
            "",
        );
        root.setSubmenu(submenu.as_ref());
    }
    app.setMainMenu(bar.as_ref());
}

ns_class!(
    /// `NSPasteboard` — the system clipboard (the general pasteboard).
    pub struct NSPasteboard : "NSPasteboard";
);

/// `NSPasteboardTypeString` — the UTI identifying plain UTF-8 text on a
/// pasteboard.
pub const NSPASTEBOARD_TYPE_STRING: &str = "public.utf8-plain-text";

impl NSPasteboard {
    /// `+[NSPasteboard generalPasteboard]` (autoreleased +0 → retained) —
    /// the clipboard every app shares.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn generalPasteboard() -> Strong<NSPasteboard> {
        Strong::from_retain(send::<Raw>(
            NSPasteboard::class_ptr().ptr(),
            ns_sel!("generalPasteboard"),
        ))
    }

    /// `-stringForType:` (autoreleased +0 → retained; nil when the
    /// pasteboard holds no text of that type).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn stringForType(&self, data_type: Ref<NSString>) -> Strong<NSString> {
        Strong::from_retain(send1::<Raw, Raw>(
            self.as_raw(),
            ns_sel!("stringForType:"),
            data_type.as_raw(),
        ))
    }

    /// `-clearContents` — drops every declared type, ready for a new
    /// write. Returns the change count.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn clearContents(&self) -> NSInteger {
        send::<NSInteger>(self.as_raw(), ns_sel!("clearContents"))
    }

    /// `-setString:forType:` — writes text under `NSPasteboardTypeString`.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setString_forType(
        &self,
        string: Ref<NSString>,
        data_type: Ref<NSString>,
    ) -> ObjBOOL {
        send2::<Raw, Raw, ObjBOOL>(
            self.as_raw(),
            ns_sel!("setString:forType:"),
            string.as_raw(),
            data_type.as_raw(),
        )
    }
}

ns_class!(
    /// `NSView` — used as a layer-hosting container for the `CAMetalLayer`.
    pub struct NSView : "NSView";
);

impl NSView {
    /// `-setWantsLayer:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setWantsLayer(&self, flag: bool) {
        send1::<ObjBOOL, ()>(self.as_raw(), ns_sel!("setWantsLayer:"), bool_arg(flag));
    }

    /// `-layer` (opaque `CALayer*`).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn layer(&self) -> Raw {
        send::<Raw>(self.as_raw(), ns_sel!("layer"))
    }

    /// `-setLayer:`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn setLayer(&self, layer: Raw) {
        send1::<Raw, ()>(self.as_raw(), ns_sel!("setLayer:"), layer);
    }

    /// `-bounds` (32-byte `NSRect` — `objc_msgSend_stret` on x86_64).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn bounds(&self) -> NSRect {
        send_stret::<NSRect>(self.as_raw(), ns_sel!("bounds"))
    }
}

ns_class!(
    /// `NSEvent` — keyboard, mouse and modifier events.
    pub struct NSEvent : "NSEvent";
);

impl NSEvent {
    /// `-type`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn eventType(&self) -> NSUInteger {
        send::<NSUInteger>(self.as_raw(), ns_sel!("type"))
    }

    /// `-keyCode` (`unsigned short` — read through `NSUInteger`, truncated).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn keyCode(&self) -> u16 {
        send::<NSUInteger>(self.as_raw(), ns_sel!("keyCode")) as u16
    }

    /// `-characters` (autoreleased +0 → retained; may be nil).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn characters(&self) -> Strong<NSString> {
        Strong::from_retain(send::<Raw>(self.as_raw(), ns_sel!("characters")))
    }

    /// `-locationInWindow` (16-byte `NSPoint` — registers).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn locationInWindow(&self) -> NSPoint {
        send::<NSPoint>(self.as_raw(), ns_sel!("locationInWindow"))
    }

    /// `-modifierFlags`
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn modifierFlags(&self) -> NSUInteger {
        send::<NSUInteger>(self.as_raw(), ns_sel!("modifierFlags"))
    }

    /// `+[NSEvent keyEventWithType:location:modifierFlags:timestamp:windowNumber:context:characters:charactersIgnoringModifiers:isARepeat:keyCode:]`
    /// (+0 autoreleased → retained). `key_code` is a HID usage code.
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn key_event(
        event_type: NSUInteger,
        location: NSPoint,
        window_number: NSInteger,
        characters: &str,
        key_code: u16,
    ) -> Strong<NSEvent> {
        let chars = NSString::from_str(characters);
        let raw = send10::<
            NSUInteger,
            NSPoint,
            NSUInteger,
            CFTimeInterval,
            NSInteger,
            Raw,
            Raw,
            Raw,
            ObjBOOL,
            u16,
            Raw,
        >(
            NSEvent::class_ptr().ptr(),
            ns_sel!(
                "keyEventWithType:location:modifierFlags:timestamp:windowNumber:context:characters:charactersIgnoringModifiers:isARepeat:keyCode:"
            ),
            event_type,
            location,
            0,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
            window_number,
            std::ptr::null_mut(),
            chars.as_ref().as_raw(),
            chars.as_ref().as_raw(),
            bool_arg(false),
            key_code,
        );
        Strong::from_retain(raw)
    }

    /// `+[NSEvent mouseEventWithType:location:modifierFlags:timestamp:windowNumber:context:eventNumber:clickCount:pressure:]`
    /// (+0 autoreleased → retained).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn mouse_event(
        event_type: NSUInteger,
        location: NSPoint,
        window_number: NSInteger,
    ) -> Strong<NSEvent> {
        let raw = send9::<
            NSUInteger,
            NSPoint,
            NSUInteger,
            CFTimeInterval,
            NSInteger,
            Raw,
            NSInteger,
            NSInteger,
            f32,
            Raw,
        >(
            NSEvent::class_ptr().ptr(),
            ns_sel!(
                "mouseEventWithType:location:modifierFlags:timestamp:windowNumber:context:eventNumber:clickCount:pressure:"
            ),
            event_type,
            location,
            0,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
            window_number,
            std::ptr::null_mut(),
            0,
            1,
            1.0f32,
        );
        Strong::from_retain(raw)
    }
}

ns_class!(
    /// `NSDate` — used only to pace the manual event loop.
    pub struct NSDate : "NSDate";
);

impl NSDate {
    /// `+[NSDate distantPast]` (autoreleased +0 → retained).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn distantPast() -> Strong<NSDate> {
        Strong::from_retain(send::<Raw>(
            NSDate::class_ptr().ptr(),
            ns_sel!("distantPast"),
        ))
    }

    /// `+[NSDate dateWithTimeIntervalSinceNow:]` (autoreleased +0 → retained).
    ///
    /// # Safety
    /// Calls into the Objective-C runtime.
    pub unsafe fn dateWithTimeIntervalSinceNow(seconds: CFTimeInterval) -> Strong<NSDate> {
        Strong::from_retain(send1::<CFTimeInterval, Raw>(
            NSDate::class_ptr().ptr(),
            ns_sel!("dateWithTimeIntervalSinceNow:"),
            seconds,
        ))
    }
}

/// `-contentsScale` on a `CALayer` (`CAMetalLayer` inherits it).
///
/// # Safety
/// Calls into the Objective-C runtime.
pub unsafe fn layer_contents_scale(layer: Raw) -> CGFloat {
    send::<CGFloat>(layer, ns_sel!("contentsScale"))
}

/// `-setContentsScale:` on a `CALayer` (`CAMetalLayer` inherits it).
///
/// # Safety
/// Calls into the Objective-C runtime.
pub unsafe fn set_layer_contents_scale(layer: Raw, scale: CGFloat) {
    send1::<CGFloat, ()>(layer, ns_sel!("setContentsScale:"), scale);
}
