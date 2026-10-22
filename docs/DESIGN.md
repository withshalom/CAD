# Design

Living notes on how the app looks and behaves. More will be added as the
design develops. Each item is marked:

- **Decided**: the owner's call.
- **Proposed**: recommended, waiting for a decision.
- **Open**: still to be discussed.

The first workspace mockups (desktop and phone, 2026-09-30) show a plate
drawn to these rules: a top view and a front view, dimensions and a note.
The core now draws that plate itself (`cad sample`, then `cad png`); §13
lists what is built.

This document is the specification for the code. Tests read the tables
in §2, §3, §4, §5 and §7 and fail when the code disagrees, so a change
here shows up as a failing test until the code follows.

## 1. Principles

- **CAD, not CAM** (Decided). Layers describe drafting meaning: visible
  outlines, centre lines, hidden lines, dimensions. They never describe
  machine operations such as cut or etch. Which layers a plasma, laser or
  router job uses is an export setting, added later, not part of the
  drawing.
- **The drawing comes first.** The canvas gets the space; panels and
  controls stay dark and quiet around it.
- **Colour carries meaning.** Each default layer has its own colour.
  Interaction colours (selection, snaps) are reserved: no default layer
  uses them, and they never print.
- **One design on every device.** Desktop, tablet and phone share the same
  layers, colours, snaps and commands; only the layout changes.
- **Faithful to DWG and DXF.** Every visual property maps to a DWG/DXF
  property (colour index, linetype, lineweight), so drawings look the same
  after a round trip through other CAD programs.

## 2. Canvas

| Item | Value | Status |
|---|---|---|
| Background | near-black `#0D1013` | Decided |
| Drafting lines | white on the black background | Decided |
| Grid | minor lines every 10 units `#151A1F`, every 5th line brighter `#1D242B` | Proposed |
| Grid toggle | GRID on/off in the status bar and on phones | Decided |
| Crosshair (desktop) | full-width crosshair with a small pick box | Proposed |
| Axis icon | bottom-left, shows the current X and Y directions | Proposed |

Proposed: the grid thins out or fills in as you zoom, so it never becomes
a wall of lines: the spacing grows or shrinks by fives so lines stay 8 to
40 pixels apart. Showing the grid and snapping to it are separate
switches (see §9).

Printing is black on white: white (colour 7) prints black, as in DWG, and
the other colours print black or as set by the plot style. Paper output
(PNG, SVG, the preview inside `.blc` files) is all black by default;
printing in colour keeps the true colours, with white still black.

## 3. Default layers

Every new drawing starts with these layers. Any number of custom layers
can be added, each with its own name, colour, linetype and lineweight, and
each can be switched off, locked or kept from printing.

| Layer | Colour (DXF index) | On screen | Linetype | Lineweight | Status |
|---|---|---|---|---|---|
| `0` | 7, white | `#FFFFFF` | Continuous | Default | Required by DWG and DXF; used inside block definitions, not for drawing |
| `VISIBLE` | 7, white | `#FFFFFF` | Continuous (solid) | 0.50 mm | Decided |
| `CENTER` | 2, yellow | `#FFE14D` | Center (long-short dash) | 0.25 mm | Decided |
| `HIDDEN` | 1, red | `#FF4D4D` | Hidden (short dash) | 0.25 mm | Decided; lineweight proposed |
| `DIMENSION` | 3, green | `#3DDC84` | Continuous (thin solid) | 0.25 mm | Decided |
| `NOTES` | 4, cyan | `#3FE0FF` | Continuous | 0.25 mm | Decided |

- **Name of the visible-line layer** (Decided). `VISIBLE`, the term ASME
  Y14.2 uses for the lines of an object you can see. It replaces the
  earlier working name `OBJECT`.
- **Every layer is customizable** (Decided). The table gives the default
  properties of a new drawing, not fixed rules. On any layer, including
  these defaults, the user can change the colour and the lineweight (and
  the linetype, and switch it on, off, lock it or keep it from printing).
  The exception is `0`, which DWG and DXF require: it cannot be deleted or
  renamed, but its properties can change too. A change applies to that
  drawing only; "save as my defaults" for new drawings is a later option.
- **Names are upper case**, as CAD convention has it; DWG layer names are
  case-insensitive.
- **Screen colours** (Proposed). Colours are stored as DXF colour numbers,
  so other programs read them correctly. On screen, red, yellow, green and
  cyan are drawn slightly softened for the black background (the hex values
  above), because the pure values look harsh as thin lines. Printing uses
  the true colours or black.
- **Lineweights** (Proposed). Drafting standards use thick and thin lines
  in a 2:1 ratio: ISO 128 pairs 0.50 with 0.25 mm; ASME Y14.2 uses about
  0.6 and 0.3 mm. The defaults above follow ISO; both values are standard
  DWG lineweights. On screen, thick lines are 2 pixels and thin lines 1
  pixel at any zoom: below 0.40 mm counts as thin, from 0.40 mm as thick,
  and 1.00 mm and heavier draw 3 pixels. Phones and high-density screens
  multiply by their pixel ratio.

## 4. Linetypes

Proposed, with proportions from ASME Y14.2, defined in the drawing's
units and scaled by the linetype scale:

| Linetype | Pattern (mm) | Rules |
|---|---|---|
| Center | long 12, gap 1.5, short 3, gap 1.5 | Centre lines cross at short dashes, extend about 3 mm past the feature, and start and end with long dashes. The app centres the pattern on each feature so this happens by itself. |
| Hidden | dash 3, gap 1.5 | Hidden lines start and end with a dash touching the visible line. |

Inch drawings use the same proportions in inches: Center 0.5, 0.0625,
0.125, 0.0625; Hidden 0.125, 0.0625.

These definitions are the project's own. Drawings opened from DWG or DXF
keep their stored linetype definitions exactly. (The DWG reader reads
linetype patterns as the ODA specification describes them, not yet
checked against real files. A linetype record it cannot read keeps only
its name; then `CENTER` and `HIDDEN` draw with the patterns above,
`CENTER2`/`HIDDEN2` at half size and `CENTERX2`/`HIDDENX2` at double size,
and other names solid.)

How the pattern lands on a feature: with n whole periods fitting in its
length, the two end dashes share what is left, so a feature starts and
ends with a dash at least half a long dash long, and the pattern is
symmetric about its middle. A feature shorter than one period is drawn
solid (a short centre line across a small hole, for instance). Each
segment of a polyline is a feature of its own, so dashes meet at corners.
Circles fit a whole number of periods, with a dash centred on 0°. The
pattern is multiplied by `$LTSCALE`, the object's own linetype scale and
any block scale.

## 5. Interaction colours

Proposed. Reserved for interaction; no default layer uses them and they
never print.

| Use | Colour | Behaviour |
|---|---|---|
| Selection | blue `#3D8BFF` | Selected objects are redrawn in blue, with square grips (8 px) on their defining points: a circle's centre and quadrants, a line's ends and middle. Dragging a grip edits the object. |
| Hover (preselection) | blue glow, 35% | The object under the cursor glows before you click, so you see what a click will pick. |
| Snap markers and tracking | magenta `#FF4FD8` | The marker's shape tells the snap type and a tag names it (§9). Tracking paths are dotted magenta. |
| Interface accent | amber `#F2A93B` | Controls only, never on the canvas: the active tool, the running command's name. |

Why not yellow or cyan: yellow is the Center layer and cyan the Notes
layer. Blue and magenta are unused by the default layers, and blue stays
clear for red-green colour-blind users.

## 6. Text

Proposed.

- **Dimension text belongs to the dimension.** It is part of the
  dimension object, lives on `DIMENSION` and is green like the rest of the
  dimension. A dimension style can give the text its own colour for anyone
  who prefers white dimension text.
- **Everything else goes on `NOTES`:** general notes, part numbers,
  labels and title block text. Notes can then be hidden or restyled without
  touching dimensions.
- **Text height** is set as it should print (3.5 mm or 1/8 in are typical)
  and multiplied by the drawing scale in model space. Later: annotative
  scaling, where one note shows at the right size at every drawing scale.
- **Dimension text reads horizontally** (unidirectional, the ASME Y14.5
  default), set into a break in the dimension line. Aligned text is an
  option.
- **Fonts.** Autodesk's SHX fonts can't be shipped. The app gets its own
  single-stroke engineering font, used by default and wherever a drawing
  asks for an SHX font. Single-stroke text also engraves and marks
  cleanly. TrueType fonts come from the operating system. Built: printable
  ASCII, the symbols drawings use (° ± ⌀ × µ ² ³), accented Latin letters;
  digits share one width so columns line up.
- **Default text style** (Proposed). New drawings get a style named
  `Standard` that names `simplex.shx`: other CAD programs draw it with
  their own Simplex font, and this app draws its own font for any SHX
  name, so the file never has to be found.

## 7. Dimensions

Decided: thin solid green lines, on the `DIMENSION` layer.

Proposed default style:

| Setting | Millimetre drawings | Inch drawings |
|---|---|---|
| Text height | 3.5 mm | 0.125 in |
| Arrowheads | filled, 3 mm long, 3:1 length to width | filled, 0.125 in long, 3:1 |
| Extension line gap from the object | 1.5 mm | 0.0625 in |
| Extension line past the dimension line | 2 mm | 0.125 in |
| Spacing between stacked dimensions | 10 mm | 0.375 in |
| Gap around text in the dimension line | 1 mm | 0.04 in |
| Decimal places | 2 | 3 |
| Dual units | optional, as `25.40 [1.000]` | optional, as `1.000 [25.40]` |

Other drawing units take these sizes converted (a metre drawing gets
0.0035 m text). With unidirectional text, stacked vertical dimensions
need more room than 10 mm when their text is long; space them so the
texts clear each other.

Proposed: when the text does not fit between the extension lines it moves
outside, past the second one, and the dimension line runs on to it; when
the arrows do not fit either, they point in from outside. Text placed by
hand stays where it was put, and the line is extended to it when it sits
on the line's path. Radius dimensions, and diameters too small for their
text or with text put outside the circle, use a leader with a short
horizontal landing before the text (`4X ⌀13.00 THRU`). Built: linear, aligned, radius and diameter;
angular and ordinate dimensions are not drawn yet.

## 8. Units

Proposed: **store coordinates in the drawing's own units; don't convert
everything to millimetres.**

- Each drawing declares its unit (DWG and DXF: `$INSUNITS`, and
  `$MEASUREMENT` for metric or imperial defaults). The core already stores
  both.
- Nothing is converted on save. A 1/8 in hole stays 0.125 in the file, and
  a 10 mm hole stays 10.
- Units convert only at the edges:
  - when you type a value with a unit (`10mm`, `3/8"` or `1'-2 1/2"` in a
    drawing of the other unit);
  - in the display format;
  - when a drawing of one unit is inserted into a drawing of another.
- Why not always millimetres: 25.4 has no exact binary value, so inch
  values stored as millimetres come back as 0.12499999999999999. That
  breaks exact round trips with DWG and DXF, and the core tests those
  round trips byte for byte. Inch fractions (1/2, 1/64) are exact in the
  file's binary numbers when kept in inches.
- It is also what binary DXF does: coordinates are unitless 8-byte numbers,
  and the header names the unit. The `.blc` format already works this way.
- Display format and precision are per drawing: decimal, fractional (to
  1/256), feet and fractional inches (architectural), feet and decimal
  inches (engineering), or scientific (`$LUNITS`, `$LUPREC`). A drawing
  with no unit shows feet and inches as if its unit were the inch.
- New drawings start from a template, Metric (mm) or Imperial (in). The
  default is chosen at first run. Proposed template settings: Metric shows
  decimals to 2 places with A3 limits (420 × 297); Imperial shows decimals
  to 3 places with ANSI B limits (17 × 11). Both start on the `VISIBLE`
  layer.
- Typed values (built): plain numbers are in the drawing's unit;
  `10mm`, `2.5 cm`, `1m`, `3/8"`, `1.5in`, `2ft`, `1'`, `1'-2"`,
  `1'-2 1/2"`, `1' 2-1/2"`, mixed numbers `1 1/2` and `1-1/2`, and
  fractions `3/8` are understood, and so is every unit's display mark
  (`mm`, `µm`, `km`, `ft (US survey)` …). What the display shows can be
  typed back. Converting between metric and imperial is exact for typed
  fractions of an inch: 76.2 mm is 3 in, not 3.0000000000000004.
- Exports state their units. Some nesting and cutting software ignores
  them, so the export shows the unit plainly, to prevent the classic
  "part came out 25.4 times too big" error.
- Snapping and matching tolerances scale with the unit.

## 9. Snaps and drawing aids

Proposed.

**Object snaps** pull the cursor onto exact points of existing geometry.
Several stay on at once ("running snaps"), and one can be forced for a
single pick by typing its name (`END`, `MID`, `CEN`, …) or from the
right-click menu.

| Snap | Marker | Point |
|---|---|---|
| Endpoint | square | end of a line, arc or polyline segment |
| Midpoint | triangle | middle of a segment or arc |
| Center | circle | centre of a circle, arc or ellipse |
| Geometric center | circle with a dot | centroid of a closed polyline |
| Quadrant | diamond | 0°, 90°, 180°, 270° points of a circle |
| Intersection | X | where two objects cross |
| Extension | small + on a dotted path | along the continuation of a line or arc |
| Perpendicular | right-angle mark | the foot of a perpendicular |
| Tangent | circle with a line | a tangent point on a circle or arc |
| Nearest | hourglass | the closest point on any object |
| Node | circle with an X | a point object |
| Insertion | two squares | the insertion point of text or a block |
| Parallel | two slanted lines | along a direction parallel to a picked line |

- Candidates are searched within an aperture: 10 px with a mouse, 22 px on
  touch. The closest one wins, Tab cycles through the others, and the tag
  names the snap.
- **Tracking:** hover a snap point for a moment to acquire it. Dotted
  magenta paths then run through it horizontally, vertically and at the
  polar angles, so a point can line up with points drawn earlier.
- **GRID** shows the grid; **SNAP** makes the cursor step by the snap
  spacing. They are separate switches.
- **ORTHO** locks drawing to the X and Y directions. **POLAR** tracks set
  angle steps (15°, 30°, 45° or 90°). The two are mutually exclusive.
- **Rotation:** ortho and polar follow the current coordinate system
  (UCS), which can be moved and rotated. "Align to object" picks a line
  and rotates X to match it. The grid, snap steps, ortho and the axis icon
  all follow. Rotating the view is separate. On phones, two-finger rotation
  of the view is off by default, so it doesn't happen by accident.
- Snap calculations live in the Rust core, shared by every platform: a
  spatial index and a "snaps near this point" query.

## 10. Desktop workspace

Proposed, as in the mockup.

- **Top bar:**
  - open drawings as tabs; a green dot means saved;
  - `+` for a new drawing;
  - undo and redo;
  - save state;
  - who has the drawing open (the `.bcl` lock), for example "Editing on
    SHOP-PC". Clicking it shows the lock's holder and time.
- **Tool rail (left):**
  - Select;
  - draw: Line, Polyline, Circle, Arc, Rectangle, Text;
  - annotate: Dimension, Measure;
  - modify: Move, Rotate, Mirror, Offset, Trim, Fillet.

  A tool starts the same command as typing it (`L`, `P`, `C`, `A`, `R`,
  `MT`, `D`, `ME`, `M`, `RO`, `MI`, `O`, `T`, `F`) and runs until Esc.
  Enter or Space repeats the last command. The active tool is amber. The
  full command set, aliases and keys are in `COMMANDS.md`.
- **Canvas overlays:**
  - top left: the current space, scale and units (click to change);
  - top right: zoom to extents, zoom in, zoom out, pan (the mouse wheel
    zooms and the middle button pans anyway);
  - bottom left: the axis icon;
  - at the cursor: the snap tag and coordinate entry boxes. Type a value,
    and Tab moves to the next.
- **Properties panel:** the selected object's layer, colour, linetype and
  lineweight, then its geometry. Values are edited in place; the Radius row
  in the mockup is being edited. Derived values (diameter, circumference,
  area) update live.
- **Layers panel:** colour, name, a linetype sample, object count and
  visibility for each layer, plus lock and print settings. The current
  layer is marked, and `+` adds a layer.
- **Drawing panel:** units, display format, scale.
- **Command line:** history above, the current prompt below. Options in
  brackets can be clicked or typed by their capital letters.
- **Status bar:** cursor coordinates; toggles for GRID, SNAP, ORTHO,
  POLAR, OSNAP and DYN (coordinate entry at the cursor); the Model and
  sheet tabs.

## 11. Phone and tablet

Decided direction: the owner likes the simplified phone layout.

- **The drawing fills the screen.**
- **Top bar:** menu, drawing name and state, undo, redo, layers.
- **Aid chips:** SNAP, ORTHO and GRID toggles, plus the coordinates of the
  current point.
- **Bottom sheet:** a swipeable strip of tools, then the selected object's
  key values as large fields. Tapping a field opens a number pad. Swipe the
  sheet up for all properties.
- **Placing a point:** a magnifier appears above the finger, so the snap
  stays visible under the thumb. Lifting the finger places the point.
- **Long-press** on an object opens a context menu.
- **Tablets** use the desktop layout, with the command line hidden until
  asked for.

## 12. Open questions

- Default unit for new drawings, or asking at first run.
- Dimension text colour: green like the dimension, or white.
- Lineweight of hidden lines (0.25 or 0.35 mm).
- Whether phones show a command line at all.
- Whether drawings carry machine operations after all. §1 says no (cut
  and etch roles, kerf, lead-ins, pierce points and cut order belong to
  the job, not the drawing), while FORMAT-ROADMAP §7 lists them as data
  the native format could hold. Format 2.0 follows §1: it stores what a
  part is made of (material, grade, thickness, quantity; Proposed) and
  measures it for a quote on request (`cad takeoff`), on the layers the
  caller names.

## 13. In the code

Stage 1 (2026-10-01): the rules above that the core can carry are built
and tested. Stage 2 (2026-10-08): the workspace itself, built as the
browser workbench (`apps/web`, an installable PWA) to the workspace
mockups.

| Item | Where | State |
|---|---|---|
| Default layers, linetypes, text and dimension styles, canvas and interaction colours | `cad_core::standards` | Built; tests compare it with §2–§5 and §7 |
| New drawings from the Metric and Imperial templates | `cad_core::new_drawing`, `cad new` | Built |
| Every layer customizable: colour, linetype, lineweight, name, state; layer `0` kept | `Drawing::add_layer`, `set_layer_*`, `rename_layer`, `delete_layer` | Built |
| Linetype patterns, centred on each feature | `render::dash` | Built |
| Lineweights in pixels, screen colours, paper colours | `standards::screen_px`, `screen_color`, `print_color` | Built |
| Dimensions from the dimension style | `render::dim` | Linear, aligned, radius, diameter |
| Own single-stroke font, text alignment, MTEXT wrapping | `cad_core::font` | Built |
| Units: conversion, typed lengths and angles, display formats, dimension text | `cad_core::units`, `cad length` | Built |
| The canvas: dark background, grid, axis icon, anti-aliased lines | `cad_core::raster`, `cad png` | Built (software, every core) |
| SVG and DXF output with linetypes | `export::svg`, `export::dxf` | Built |
| The sample plate on the default layers | `cad_core::samples::plate`, `cad sample` | Built |
| Drawing properties (title, author, revision, custom properties) | `HeaderVars::properties`, `cad props` | Built (format 2.0) |
| What a part is made of, and its length, contours, area and weight | `BlockHeader::part`, `cad part`; `cad_core::takeoff`, `cad takeoff` | Proposed (format 2.0) |
| Commands, aliases and keys | `COMMANDS.md` | Alias rule Decided; assignments Proposed; the underlying core work marked per command |
| Snaps (§9): endpoint, midpoint, center, quadrant, intersection, nearest, node, insertion; markers and tags; aperture 10 px mouse, 22 px touch; Tab cycles; typed snap keywords | `cad_core::snap`, `apps/web` | Built (straight pass; spatial index later). Perpendicular, tangent, extension, parallel and tracking still to do |
| Ortho, polar (15° steps), grid stepping, grid display spacing by fives | `apps/web` | Built. UCS rotation still to do |
| Selection (§5): blue selection, hover glow, square grips that edit by dragging, window and crossing boxes | `cad_core::snap`, `apps/web` | Built |
| Workspace, panels, command line, status bar (§10) | `apps/web` (browser workbench, PWA) | Built to the mockup. Presence shows "this browser" until a desktop shell can hold a `.bcl` lock |
| Phone layout (§11): title bar, aid chips, tool sheet, value fields with a number pad, magnifier while placing a point | `apps/web` | Built. Long-press menu still to do |

### Stage 3 (2026-10-15): the drafting tools live in the core

Every tool below does its geometry in `cad_core` and is reached through one
session command line (`cad-ffi`, `commands.rs` and `session.rs`). The web
app only prompts for points, draws the rubber band, and sends the line.

| Item | Where | Command line |
|---|---|---|
| Offset, trim, extend, fillet, chamfer, scale, stretch, arrays | `modify`, `boundary`, `ops` | `offset`, `trim`, `extend`, `fillet`, `chamfer`, `scale`, `scale-ref`, `stretch`, `array-rect`, `array-polar` |
| Arc through three points; linear, aligned, radial dimensions from picked points; MTEXT in a box; construction lines at an angle; distance and angle readout; array spacing | `construct` | `arc-3p`, `dim-linear`, `dim-aligned`, `dim-radius`, `dim-diameter`, `mtext-box`, `xline-angle`; queries `measure`, `bounds` |
| Grips: what each object shows and what dragging does | `grips` | `grip`; query `grips` |
| Hatch, construction lines, leaders, angular, baseline and continued dimensions | `render::hatch`, `ops`, `export::dimblocks` | `hatch`, `hatch-objects`, `xline`, `ray`, `leader`, `dim-angular*`, `dim-baseline`, `dim-continue` |
| Blocks and attributes | `blocks` | `block`, `insert`, `attdef`, `attedit` |
| Units, linetypes, lineweights, layer plot flag | `units`, `standards`, `model` | `units`, `linetype-*`, `ltscale`, `lineweight-current`, `layer-plot` |
| Layouts, viewports, paper and PDF plot | `layouts`, `export::pdf`, `render::plotted` | `layout-*`, `page-setup`, `mview`; save as `pdf` |
| A sheet ready to plot: layout, border, viewport at a standard scale, title block with fill-in fields | `sheet` | `sheet` |
| Hole check against shop limits | `check` | query `check-holes` |

Not in the core, by design: ghost previews while dragging, prompts, and the
lessons on the Blackboard (`apps/web` and the single-file page).
