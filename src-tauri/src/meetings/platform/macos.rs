//! macOS's way of moving a meeting folder to the Trash (TUR-116).

use trash::macos::{DeleteMethod, TrashContextExtMacos as _};

/// macOS: `NSFileManager`'s `trashItemAtURL`, not the crate's default of
/// asking Finder over Apple Events. The Finder way makes macOS ask the user
/// to let meet-ai control Finder the first time, a prompt this app has no
/// other reason to show. The trade-off, from the `trash` crate's docs: on
/// some macOS versions the Trash then offers no "Put Back" for the folder,
/// and it is dragged out of the Trash instead.
pub(super) fn configure(context: &mut trash::TrashContext) {
    context.set_delete_method(DeleteMethod::NsFileManager);
}
