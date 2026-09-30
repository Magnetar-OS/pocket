app-title = Pocket
# Also the desktop entry's Comment and the AppStream <summary>, via build.rs.
app-comment = Boarding passes, tickets and cards, with their barcodes ready to scan
# Semicolon-separated, matching the desktop-entry convention. build.rs splits
# them into one <keyword> element each.
app-keywords = wallet;pass;passes;pkpass;boarding;ticket;barcode;loyalty;card;coupon;

## Shell

about = About
settings = Settings
repository = Repository

## The sidebar

all-passes = All passes
boarding-passes = Boarding passes
event-tickets = Event tickets
store-cards = Cards
coupons = Coupons
generic-passes = Other

## The list

no-passes = No passes yet
no-passes-detail = Passes arrive as .pkpass files — from an airline's confirmation email, an event booking, or a loyalty scheme. Add one with “Add pass…”, drop it on this window, or open it from your file manager. They are kept in { $path }.
passes-count = { $count } { $count ->
        [one] pass
       *[other] passes
    }
select-a-pass = Select a pass
# Above a pass opened from a file rather than from the wallet.
opened-from-file = Opened from a file. This pass is shown, not kept in your wallet.

## Adding and removing

# The header button that opens the file dialog, and that dialog's title.
add-pass = Add pass…
# The file dialog's filter.
pass-files = Passes (.pkpass, .pkpasses)
# Shown while files are being dragged over the window.
drop-to-add = Drop to add to your wallet
# The button beside a pass opened from a file.
add-to-wallet = Add to wallet
added = Added to your wallet.
added-updated = Your wallet already had an earlier version of this pass. It has been replaced with this one.
added-already = This pass is already in your wallet.
added-several = { $count } passes added to your wallet.
add-failed = { $name } could not be added: { $reason }
no-wallet = the wallet could not be opened
remove-from-wallet = Remove from wallet
remove-title = Remove { $name } from your wallet?
remove-body = The pass is deleted from this computer. If you have no other copy of it, it cannot be brought back.
remove = Remove
cancel = Cancel
removed = { $name } was removed from your wallet.
remove-failed = { $name } could not be removed: { $reason }

## A pass

expired = Expired
voided = Voided by the issuer
unreadable-passes = { $count } { $count ->
        [one] pass could not be read
       *[other] passes could not be read
    }
# One line per pass that would not read: its folder name, then why.
unreadable-pass = { $id }: { $reason }
back-of-pass = Back of pass

## The barcode

# The button that puts the barcode on the whole screen.
present = Show barcode
done = Done
# The reason given to the desktop for keeping the screen awake and bright.
presenting-a-pass = A pass is being shown for scanning
barcode-unavailable = The barcode cannot be shown
