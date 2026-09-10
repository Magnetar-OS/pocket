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
no-passes-detail = Passes arrive as .pkpass files — from an airline's confirmation email, an event booking, or a loyalty scheme. Drop one into { $path }.
passes-count = { $count } { $count ->
        [one] pass
       *[other] passes
    }
select-a-pass = Select a pass

## A pass

expired = Expired
voided = Voided by the issuer
unreadable-passes = { $count } { $count ->
        [one] pass could not be read
       *[other] passes could not be read
    }
back-of-pass = Back of pass

## The barcode

# The button that puts the barcode on the whole screen.
present = Show barcode
done = Done
# The reason given to the desktop for keeping the screen awake and bright.
presenting-a-pass = A pass is being shown for scanning
barcode-unavailable = The barcode cannot be shown
