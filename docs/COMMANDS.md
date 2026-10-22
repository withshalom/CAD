# Commands and keyboard shortcuts

The full command set the editor will offer, with typed aliases and keys.
Every device runs the same commands (DESIGN.md §1): the desktop tool rail,
the phone's tool sheet and the command line all start the same thing.

Status: the **alias rule** (§2) is Decided, and so are the **single-letter
assignments** in §3: the owner approved them on the condition that no two
commands in this program want the same alias, which was checked (none do).
The rest of the assignments, and the familiar secondary aliases in §3a, are
Proposed.

Columns in the tables:

- **Alias**: what you type, then Space or Enter. `—` means the full name
  only (rare commands don't spend a short alias).
- **Keys**: a key or key combination that runs it directly.
- **Phase**: 1 = the first editor release, 2 = the release after,
  L = later.
- **Today**: what in the core already does the underlying work. Blank
  means it is still to build. The interactive command itself (prompts,
  preview, grips) is still to build for every row.
  - `cad …`: a command of the test build does it now.
  - core API: the core's Rust API does it (for draw commands: it can
    create that kind of object; `Drawing::add_entity`).
  - renders / reads: the core draws it, or reads it from DWG, but cannot
    yet make or change it.
  - standards, font, units, takeoff, recovery, change sets: the module
    that supplies the defaults or the maths is built.

## 1. How commands work

- **Every tool is a command with a full name** (`LINE`, `OFFSET`). Typing
  the full name always works; an alias is only a shortcut. Case doesn't
  matter; names and aliases are shown in capitals.
- **Typing goes to the command line from anywhere.** There is no need to
  click it first. A list of matches appears as you type (`L` shows LINE,
  LA, LAM, LE …); Up and Down pick from it.
- **An alias runs on Space or Enter**, not on the keystroke, because short
  aliases are prefixes of longer ones (`LA`, `LAM`). Space is the big key
  under the left thumb, so the hand never leaves the keyboard.
- **Space or Enter at an empty prompt repeats the last command.** Esc
  cancels the command, then clears the selection.
- **Options** appear in brackets, for example `[Close/Undo/Arc]`. The
  capital letter of each option is its key: type it, or click the option.
  The key is the option's first letter unless two options share it, as in
  `[Halfwidth/Width]`.
- **Noun–verb and verb–noun both work:** select first then start a modify
  command, or start it and it asks what to select.
- **One command, one operation.** A command that changes the drawing
  finishes as a single operation: one undo step, one entry in the next
  change set (FORMAT.md, incremental save) and, later, one message in live
  collaboration. A cancelled command records nothing. Viewing and
  measuring commands change nothing and record nothing.
- **Transparent commands** run in the middle of another command when
  prefixed with an apostrophe: `'Z`, `'PA`, `'DI`, `'CAL`, `'GR`, `'OS`.
  The interrupted command carries on afterwards.

## 2. The alias rule (Decided)

1. Aliases are capital letters.
2. **The most-used command beginning with a letter gets that letter.**
3. The next most-used gets **two letters**: the first two letters of its
   name, or its first letter and the next distinctive letter when the
   first two are taken (`DR` dimension radius, `DD` diameter).
4. Less-used commands get **three letters**, by the same method.
5. **Families share a prefix:** `D…` dimensions, `LA…` layer tools, `Z…`
   zoom. Learning one member teaches the pattern.
6. An alias is never the full name of a different command.
7. Commands with a standard Ctrl key (open, save, copy to clipboard …)
   don't spend an alias, except `U` for undo, which is typed constantly
   between other commands.
8. Letters with no command left free: `K`, `N`, `Y`. They are kept for the
   user's own aliases.
9. Every alias can be changed per user (`ALIASEDIT`). The tables here are
   the defaults that ship.
10. A command may also accept **secondary aliases** that people know from
    elsewhere (§3a), wherever they are free. The rule decides the primary.

## 3. Contested letters (Decided)

Where two common commands want the same letter, frequency in 2D part and
fabrication drafting decides. Each of these letters belongs to exactly one
command in this program, so none of them is an internal conflict (checked
across every alias in this document: no alias is used twice, none is another
command's full name, and each begins with its command's first letter). They
differ only from the habits of some other CAD programs, covered in §3a.
Each row can still be flipped without touching anything else.

| Letter | Gets it | Runner-up | Why |
|---|---|---|---|
| P | PLINE | PAN `PA` | Panning is done with the middle button, the wheel or two fingers; polylines (plate outlines) are typed constantly. |
| R | RECTANGLE | ROTATE `RO` | Most plates start as a rectangle. |
| S | STRETCH | SCALE `SC` | Parts are resized by stretching far more than by scaling; saving is Ctrl+S. |
| T | TRIM | TEXT `TE`, MTEXT `MT` | Trim runs dozens of times per drawing; notes a few. |
| D | DIM | DIST `DI` | One smart DIM picks linear, aligned, radius, diameter or angular from what you click. |
| E | ERASE | EXTEND `EX`, EXPLODE `EXP` | The Delete key also erases a selection. |
| C | CIRCLE | COPY `CO` | Holes. |
| M | MOVE | MIRROR `MI`, MATCHPROP `MA`, MTEXT `MT` | |
| A | ARC | ARRAY `AR`, AREA `AA` | |
| X | XLINE | XREF `XR` | By the rule. Explode lives under E. |

These replace the aliases proposed in DESIGN.md §10 before this document
(`PL`, `REC`, `TR`, `T` for text, `MEA`).

## 3a. Familiar aliases (Proposed)

People arrive with habits from other CAD programs. These are recalled from
general familiarity with the common conventions, not from any program's
alias file. The rule in §2 stays the primary, and the program can also
accept a **secondary alias** for a command wherever that alias is free.
Secondaries run the same command, show next to the primary in the command
search, and follow the rule that an alias is never another command's full
name.

**Already the same** (about 60 commands), among them `L`, `C`, `A`, `M`,
`CO`, `RO`, `MI`, `SC`, `S`, `O`, `EX`, `F`, `E`, `H`, `B`, `I`, `W`, `J`,
`BR`, `AR`, `AL`, `LEN`, `PE`, `MA`, `LA`, `LT`, `UN`, `OS`, `DI`, `AA`,
`LI`, `ID`, `Z`, `U`, `XR`, `PU`, `ST`, `POL`, `EL`, `PO`, `DIV`, `ME`,
`DBA`, `DCO`, `DOR`.

**Free secondaries** (the familiar alias does not collide with anything
here, so it can run the same command):

| Command | Primary | Also accepts |
|---|---|---|
| PLINE | `P` | `PL` |
| RECTANGLE | `R` | `REC` |
| SPLINE | `SP` | `SPL` |
| COPY | `CO` | `CP` |
| TRIM | `T` | `TR` |
| TEXT | `TE` | `DT` |
| CHAMFER | `CH` | `CHA` |
| PROPERTIES | `PR` | `MO` |
| LEADER | `LE` | `LEAD` |
| XLINE | `X` | `XL` |
| DIMLINEAR | `DL` | `DLI` |
| DIMRADIUS | `DR` | `DRA` |
| DIMDIAMETER | `DD` | `DDI` |
| DIMALIGNED | `DA` | `DAL` |
| DIMEDIT | `DE` | `DED` |
| REVCLOUD | `REV` | `REVC` |

**Where the rule wins over a habit** (the familiar alias is already
another command's primary here, so it can't be a secondary):

| Habit | Elsewhere | Here |
|---|---|---|
| `P` | pan | PLINE (pan is `PA`) |
| `R` | redraw | RECTANGLE (the view redraws itself) |
| `T` | multiline text | TRIM (text is `MT`) |
| `X` | explode | XLINE (explode is `EXP`) |
| `D` | dimension style | DIM (style is `DS`) |
| `DR` | draw order | DIMRADIUS (draw order is `DRO`) |
| `CH` | properties | CHAMFER (properties is `PR` or `MO`) |

Per-user aliases (`ALIASEDIT`, §2 rule 9) let anyone make `P` mean pan,
or load a whole familiar set, without changing the defaults.

## 4. Draw

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| LINE | `L` | | Line segments, one after another. `[Close/Undo]` | 1 | core API |
| PLINE | `P` | | Polyline of lines and arcs, one object. `[Arc/Close/Halfwidth/Length/Undo/Width]` | 1 | core API |
| CIRCLE | `C` | | Circle by centre and radius. `[3P/2P/Ttr/Diameter]` | 1 | core API |
| ARC | `A` | | Arc by three points. `[Center/End/Direction/Radius]` | 1 | core API |
| COMPASS | `CPS` | | Arc the way a drafting compass draws it: plant the centre, open to a span, swing; snaps to where the swing crosses existing lines and keeps only the part between two crossings (§4a). `[Span/Full/Undo]` | 1 | core API (arcs) |
| RECTANGLE | `R` | | Rectangle by two corners or by size. `[Chamfer/Fillet/Area/Dimensions/Rotation]` | 1 | core API |
| POLYGON | `POL` | | Regular polygon. `[Edge]`, then `[Inscribed/Circumscribed]` | 1 | |
| ELLIPSE | `EL` | | Ellipse or elliptical arc. `[Arc/Center]` | 1 | core API |
| SPLINE | `SP` | | Smooth curve through or controlled by points. `[Fit/CV/Close]` | 2 | core API |
| POINT | `PO` | | Point object (a node to snap to). | 1 | core API |
| XLINE | `X` | | Construction line, infinite both ways. `[Hor/Ver/Ang/Bisect/Offset]` | 1 | |
| RAY | `RA` | | Construction line, infinite one way. | 2 | |
| DIVIDE | `DIV` | | Points or blocks at equal spacing along an object. `[Block/Length]` | 2 | |
| CENTERLINE | `CL` | | Centre line between two lines, on the centrelines layer. | 1 | |
| CENTERMARK | `CM` | | Centre mark and lines on a circle or arc, on the centrelines layer. | 1 | |
| REVCLOUD | `REV` | | Revision cloud, drawn or made from an object. | 2 | |
| WIPEOUT | `WI` | | Mask that hides what is behind it. | 2 | |
| BOUNDARY | `BO` | | Closed polyline from the area around a picked point. | 2 | |

### 4a. COMPASS (Proposed)

The owner's design: a tool that behaves like a physical drafting compass,
for layout work where arcs are built from existing geometry.

1. **Plant the point.** Pick the centre (any snap).
2. **Open the span.** Pick the distance point (any snap). That sets the
   radius, and the arc starts there, where the pencil sits.
3. **Ghost circle.** A faint full circle shows at that radius, so you can
   see where the swing will cross existing lines and arcs.
4. **Swing.** Drag around the centre; the arc follows the drag direction,
   with a live angle readout. Type an angle to end it exactly.
5. **Crossing snaps.** While swinging, the end snaps to where the ghost
   circle crosses existing geometry (a snap that exists only during this
   command, marked like Intersection).
6. **Between two crossings.** Click one crossing, then another: only the
   arc between them is kept, choosing the side the cursor passed. It does in
   one gesture what drawing a circle and trimming it would take.
7. **`Span`**: take the opening from somewhere else first: two picked
   points, a line's length or a circle's radius, or a typed length. Then
   plant the centre anywhere; the span carries over, as when a real
   compass is lifted and set down. The span is kept for the next COMPASS
   until changed, so a series of equal arcs is quick.
8. **`Full`**: keep the whole circle instead of an arc.

Result: an ordinary ARC (or CIRCLE), on the current layer; nothing
proprietary in the drawing, so it reads the same in any DWG program.

Prior art found so far: drafting programs draw arcs by centre, start and
end, and have intersection snaps and TRIM, separately; a geometry-teaching
program (GeoGebra) has a Compass tool that transfers a radius from a
segment to a new centre. No drafting tool was found that combines the span
transfer, the crossing snaps during the swing and the keep-between-two-
crossings step. The search was not exhaustive and was not a patent search.

What the core needs: the circle-and-object intersection that snaps
already use (§9 of DESIGN.md), evaluated against a circle that is not yet
an object, and an arc-between-two-angles constructor.

## 5. Annotate

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| DIM | `D` | | Smart dimension: linear, aligned, radius, diameter or angular from what you pick. | 1 | core API |
| DIMLINEAR | `DL` | | Horizontal or vertical dimension. | 1 | core API, renders |
| DIMRADIUS | `DR` | | Radius of an arc or circle. | 1 | core API, renders |
| DIMDIAMETER | `DD` | | Diameter of a circle or arc. | 1 | core API, renders |
| DIMALIGNED | `DA` | | Dimension parallel to two points. | 1 | core API, renders |
| DIMANGULAR | `DAN` | | Angle between lines, or of an arc. | 1 | core API |
| DIMBASELINE | `DBA` | | More dimensions from the first one's base. | 1 | |
| DIMCONTINUE | `DCO` | | Chain of dimensions, each from the last. | 1 | |
| DIMORDINATE | `DOR` | | X or Y distance from the origin (hole charts, CNC-style layouts). | 2 | |
| DIMARC | `DAR` | | Length along an arc. | 2 | |
| DIMJOGGED | `DJ` | | Radius with a jogged line, for large radii. | L | |
| DIMEDIT | `DE` | | Move dimension text, override its value, make it oblique. | 1 | |
| DIMSPACE | `DSP` | | Space stacked dimensions evenly. | 2 | |
| DIMBREAK | `DBR` | | Break dimension and extension lines where they cross. | L | |
| DIMSTYLE | `DS` | | Dimension styles. | 2 | standards |
| MTEXT | `MT` | | Paragraph text: notes, wrapping, on the NOTES layer. | 1 | core API, font |
| TEXT | `TE` | | Single-line text: labels. | 1 | core API, font |
| TEXTEDIT | `TED` | | Edit text or a dimension's text (double-click does the same). | 1 | |
| STYLE | `ST` | | Text styles. | 2 | standards |
| LEADER | `LE` | | Arrow and note pointing at a feature. | 1 | |
| TABLE | `TB` | | Table of rows and columns. | L | |
| HATCH | `H` | | Fill or pattern inside a boundary. `[Pick/Select/Pattern/Scale/Angle]` | 2 | reads |
| HATCHEDIT | `HE` | | Change a hatch's pattern, scale or angle. | 2 | |
| FIND | `FI` | Ctrl+F | Find and replace text. | 2 | |
| SPELL | — | | Check spelling. | L | |

## 6. Modify

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| ERASE | `E` | Delete | Remove objects. | 1 | |
| MOVE | `M` | | Move by base point and second point, or by distance. | 1 | |
| COPY | `CO` | | Copy, as many times as wanted. `[Array/Mode]` | 1 | |
| ROTATE | `RO` | | Rotate about a point. `[Copy/Reference]` | 1 | |
| MIRROR | `MI` | | Mirror across a line, keeping or erasing the source. | 1 | |
| SCALE | `SC` | | Scale about a point. `[Copy/Reference]` | 1 | |
| STRETCH | `S` | | Move the vertices inside a crossing window; the rest stays. | 1 | |
| OFFSET | `O` | | Parallel copy at a distance. `[Through/Erase/Layer]` | 1 | |
| TRIM | `T` | | Cut objects back to the nearest edges. Hold Shift to extend instead. | 1 | |
| EXTEND | `EX` | | Lengthen objects to the nearest edges. Hold Shift to trim instead. | 1 | |
| FILLET | `F` | | Round a corner with a radius (0 makes a sharp corner). `[Radius/Polyline/Trim/Multiple]` | 1 | |
| CHAMFER | `CH` | | Bevel a corner. `[Distance/Angle/Polyline/Trim/Multiple]` | 1 | |
| BREAK | `BR` | | Cut a gap out of an object. `[First point/At point]` | 1 | |
| JOIN | `J` | | Join lines and arcs end to end into one object. | 1 | |
| EXPLODE | `EXP` | | Break a block, polyline or dimension into its parts. | 1 | |
| ARRAY | `AR` | | Copies in rows and columns, around a centre, or along a path. `[Rectangular/Polar/Path]` | 2 | |
| ALIGN | `AL` | | Move and rotate (and optionally scale) by matching point pairs. | 2 | |
| LENGTHEN | `LEN` | | Change a line or arc's length. `[Delta/Percent/Total/Dynamic]` | 2 | |
| PEDIT | `PE` | | Edit polylines: join, width, vertices, close or open. | 1 | |
| SPLINEDIT | `SPE` | | Edit a spline's points and fit. | L | |
| MATCHPROP | `MA` | | Copy layer, colour, linetype and other properties from one object to others. | 1 | |
| PROPERTIES | `PR` | Ctrl+1 | The properties panel (DESIGN.md §10). | 1 | core API |
| DRAWORDER | `DRO` | | Bring objects to the front or send them back. | 2 | |
| OVERKILL | `OV` | | Remove duplicate and overlapping objects (cleans imported DWGs). | 2 | |
| NUDGE | — | Ctrl+Arrow | Move the selection one snap step. | 2 | |

## 7. Selection and groups

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| SELECTALL | — | Ctrl+A | Select everything that isn't on a locked or frozen layer. | 1 | |
| QSELECT | `Q` | | Select by type and property (all circles of 18 mm on VISIBLE). | 2 | |
| SELECTSIMILAR | `SS` | | Select objects like the ones selected (same type and layer). | 1 | |
| GROUP | `G` | | Named group that selects as one. | 2 | |
| UNGROUP | `UG` | | Dissolve a group. | 2 | |

Mouse and touch: window left to right selects what is fully inside;
right to left selects what it touches; Shift+click removes from the
selection; Tab cycles through overlapping objects (DESIGN.md §9).

## 8. Layers and properties

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| LAYER | `LA` | Ctrl+2 | The layers panel: add, rename, colour, linetype, lineweight, on/off, freeze, lock, print, current. | 1 | `cad layers`, core API |
| LAYMCUR | `LAM` | | Make the picked object's layer current. | 1 | core API |
| LAYCUR | `LAC` | | Move the selected objects to the current layer. | 1 | |
| LAYISO | `LAI` | | Show only the picked objects' layers. | 1 | |
| LAYUNISO | `LAU` | | Undo the isolation. | 1 | |
| LAYOFF | `LAO` | | Turn off the picked object's layer. | 1 | core API |
| LAYON | `LAN` | | Turn all layers on. | 1 | core API |
| LAYFRZ | `LAF` | | Freeze the picked object's layer. | 2 | core API |
| LAYTHW | `LAT` | | Thaw all layers. | 2 | core API |
| LAYLCK | `LAL` | | Lock the picked object's layer. | 2 | core API |
| LAYULK | `LAK` | | Unlock the picked object's layer. | 2 | core API |
| LAYDEL | `LAD` | | Delete a layer, moving or erasing what is on it. | 2 | core API (empty layers) |
| LAYMRG | `LMR` | | Merge layers into one. | L | |
| LAYPREV | `LAP` | | Undo the last change to layer settings only. | 2 | |
| LAYERSTATE | `LAS` | | Save and restore named sets of layer settings. | L | |
| COLOR | `COL` | | Colour for new objects (default ByLayer). | 1 | |
| LINETYPE | `LT` | | Load and choose linetypes. | 1 | standards |
| LINEWEIGHT | `LW` | | Lineweight for new objects, and whether lineweights show. | 1 | standards |
| LTSCALE | `LTS` | | Global linetype scale. | 1 | renders |

## 9. Blocks and references

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| BLOCK | `B` | | Make a block from objects and a base point. | 1 | core API (adds to existing blocks) |
| INSERT | `I` | | Place a block. `[Scale/Rotate/Explode/Repeat]` | 1 | core API |
| BEDIT | `BE` | | Edit a block's definition in place. | 2 | |
| WBLOCK | `W` | | Write a block or objects to their own drawing file. | 2 | |
| BASE | `BAS` | | The drawing's base point when inserted elsewhere. | 2 | |
| ATTDEF | `ATT` | | Attribute definition (a field a block asks for when placed). | 2 | |
| ATTEDIT | `ATE` | | Edit a placed block's attribute values. | 2 | renders |
| BCOUNT | `BC` | | Count block references, by name. | 2 | |
| XREF | `XR` | | Attach another drawing as a live reference. | L | |
| XCLIP | `XC` | | Clip a reference or block to a boundary. | L | |
| IMAGE | `IM` | | Attach a raster image (a scan, a photo of a site). | L | |
| PURGE | `PU` | | Remove unused blocks, layers, styles and linetypes. | 1 | |

## 10. Measure and inquire

These change nothing, so they never add an undo step.

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| DIST | `DI` | | Distance, angle and ΔX/ΔY between two points. | 1 | |
| MEASURE | `ME` | | Measure on hover: distance, radius, angle, area. `[Distance/Radius/Angle/Area]` | 1 | |
| AREA | `AA` | | Area and perimeter of a boundary or of objects, adding and subtracting. | 1 | takeoff |
| ID | `ID` | | Coordinates of a point. | 1 | |
| LIST | `LI` | | Everything about the selected objects, as text. | 1 | `cad dump` |
| CALC | `CAL` | | Calculator that takes lengths in the drawing's units (`1'-2 1/2" * 3`). | 2 | units |
| TAKEOFF | `TA` | | Length, contours, holes, net area and weight of a part, for a quote. `[Block/Layers]` | 1 | `cad takeoff` |
| PART | `PT` | | What a part is made of: material, grade, thickness, quantity. | 1 | `cad part` |
| BOM | `BM` | | Bill of materials: every part in the drawing with quantity and weight. | L | |

No machine-operation commands (kerf, lead-ins, pierce points, cut order):
the drawing is CAD, not CAM (DESIGN.md §1). Those belong to the export.

## 11. View

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| ZOOM | `Z` | wheel | Zoom. `[Extents/Window/Previous/Object/All]` | 1 | |
| ZOOM Extents | `ZE` | Home, double-click wheel | Fit the whole drawing. | 1 | renders |
| ZOOM Window | `ZW` | | Zoom to a picked window. | 1 | |
| ZOOM Previous | `ZP` | | Back to the last view. | 1 | |
| ZOOM Object | `ZO` | | Fit the selected objects. | 1 | |
| PAN | `PA` | middle drag | Slide the view. | 1 | |
| VIEW | `V` | | Named views: save and restore. | 2 | |
| ISOLATE | `IS` | | Show only the selected objects (temporary, not a layer change). | 2 | |
| UNISOLATE | `UI` | | Show everything again. | 2 | |
| CLEANSCREEN | — | Ctrl+0 | Hide the panels; the drawing fills the window. | 1 | |
| COMMANDLINE | — | Ctrl+9 | Show or hide the command line. | 1 | |
| NEXTDRAWING | — | Ctrl+Tab, Ctrl+Shift+Tab | Next or previous open drawing. | 1 | |

## 12. Drafting aids and settings

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| OSNAP | `OS` | F3 toggles | Running object snaps (DESIGN.md §9). | 1 | |
| GRID | `GR` | F7 toggles | Grid display and spacing. | 1 | renders |
| SNAP | `SN` | F9 toggles | Snap the cursor to a step. | 1 | |
| ORTHO | `OR` | F8 toggles | Lock to the X and Y directions of the UCS. | 1 | |
| POLAR | — | F10 toggles | Track angle steps (15°, 30°, 45°, 90°). Turns ORTHO off. | 1 | |
| TRACK | — | F11 toggles | Object snap tracking (dotted alignment paths). | 1 | |
| DYN | — | F12 toggles | Coordinate and length entry at the cursor. | 1 | |
| UCS | `UC` | | Move and rotate the drawing axes. `[Object/World/Previous/3point]` | 2 | |
| UNITS | `UN` | | Drawing units, display format and precision (DESIGN.md §8). | 1 | `cad length` |
| OPTIONS | `OP` | Ctrl+, | App settings. | 1 | |
| ALIASEDIT | — | | Change the aliases and keys in this document, per user. | 2 | |
| HELP | — | F1 | Help for the current command. | 1 | |
| HISTORY TEXT | — | F2 | The command line history as a window. | 1 | |

## 13. Layouts and output

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| LAYOUT | `LO` | | Paper layouts (sheets): new, rename, copy, delete. | 2 | |
| MVIEW | `MV` | | Viewport on a layout showing model space at a scale. | 2 | |
| PAGESETUP | `PS` | | Paper size, plot style, scale for a layout. | 2 | |
| PRINT | `PRI` | Ctrl+P | Print or make a PDF. | 2 | `cad png --light` |
| EXPORT | — | Ctrl+Shift+E | Save a copy as DXF, SVG, PNG, PDF; DWG later. Units stated plainly (DESIGN.md §8). | 1 | `cad dxf`, `cad svg`, `cad png` |
| PUBLISH | — | | Print or export a set of layouts at once. | L | |

## 14. Files and the drawing

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| NEW | — | Ctrl+N | New drawing from the Metric or Imperial template. | 1 | `cad new` |
| OPEN | — | Ctrl+O | Open a `.blc` or DWG file. | 1 | every command |
| SAVE | — | Ctrl+S | Save. Small changes append a change set; a full save writes a fresh snapshot. | 1 | `cad save`, core API |
| SAVEAS | — | Ctrl+Shift+S | Save under a new name or folder. | 1 | `cad save` |
| CLOSE | — | Ctrl+W | Close the drawing. | 1 | |
| QUIT | — | Ctrl+Q | Quit the app. | 1 | |
| UNDO | `U` | Ctrl+Z | Undo the last command. `U` also works inside LINE and PLINE for the last segment. | 1 | |
| REDO | — | Ctrl+Y, Ctrl+Shift+Z | Redo. | 1 | |
| COPYCLIP | — | Ctrl+C | Copy the selection to the clipboard. | 1 | |
| CUTCLIP | — | Ctrl+X | Cut to the clipboard. | 1 | |
| PASTECLIP | — | Ctrl+V | Paste at a picked point. | 1 | |
| COPYBASE | — | Ctrl+Shift+C | Copy with a picked base point. | 1 | |
| PASTEBLOCK | — | Ctrl+Shift+V | Paste as a block. | 2 | |
| PASTEORIG | — | | Paste at the original coordinates. | 2 | |
| DRAWINGPROPS | `DP` | | Title, author, revision, keywords, custom properties. | 1 | `cad props` |
| HISTORY | `HI` | | The drawing's saved change sets, with who and when; open the drawing as it was at any of them. | 2 | change sets |
| AUDIT | `AU` | | Check the drawing and file for errors, and fix what can be fixed. | 1 | `cad inspect` |
| RECOVER | — | | Open a damaged file, repairing it from the recovery data. | 1 | recovery |
| COMPACT | — | | Fold the change sets into a fresh snapshot (smaller file). | 1 | `cad compact` |

## 15. Working together

| Command | Alias | Keys | Does | Phase | Today |
|---|---|---|---|---|---|
| WHO | `WH` | | Who has this drawing open, where, and since when (the `.bcl` lock). Take over a stale lock. | 1 | `cad presence` |
| SHARE | — | | Invite others into a live session. | L | |
| SYNC | — | | Exchange change sets with the others in the session. | L | change sets |

## 16. Snap keywords at a point prompt

When a command asks for a point, typing one of these forces that snap for
the next pick only (DESIGN.md §9). Shift+right-click shows the same list.

| Type | Snap | | Type | Snap |
|---|---|---|---|---|
| `END` | Endpoint | | `PER` | Perpendicular |
| `MID` | Midpoint | | `TAN` | Tangent |
| `CEN` | Center | | `NEA` | Nearest |
| `GCE` | Geometric center | | `NOD` | Node |
| `QUA` | Quadrant | | `INS` | Insertion |
| `INT` | Intersection | | `PAR` | Parallel |
| `EXT` | Extension | | `NON` | None for this pick |
| `FROM` | Offset from a base point | | `MTP` | Midway between two points |
| `TT` | Temporary tracking point | | | |

These are read only at a point prompt, so they never clash with aliases.

## 17. Typing points and lengths

At a point prompt:

| Type | Means |
|---|---|
| `120` (while pointing) | 120 units in the direction of the cursor (with ORTHO or POLAR, exactly along it) |
| `50,30` | absolute X,Y |
| `@50,30` | relative to the last point |
| `@75<30` | 75 units at 30° from the last point |
| `<30` | lock the angle to 30°, then pick or type the length |
| `3/8"`, `1'-2 1/2"`, `10mm` | lengths in any unit, converted to the drawing's (DESIGN.md §8; built: `cad length`) |

With DYN on, the same values go into the boxes at the cursor; Tab moves to
the next box.

## 18. Mouse, touch and keyboard

| Input | Does |
|---|---|
| Space, Enter | Run the typed alias; finish input; at an empty prompt, repeat the last command |
| Esc | Cancel the command, then clear the selection |
| Up, Down at the command line | Recall earlier commands |
| Tab | Cycle snap candidates, or overlapping objects when selecting |
| Delete | Erase the selection |
| Wheel | Zoom about the cursor |
| Middle drag | Pan |
| Double-click wheel | Zoom extents |
| Double-click an object | Edit it (text editor, PEDIT, block editor, properties) |
| Right-click | Context menu (or Enter, as a setting) |
| Shift+right-click | Snap menu |
| Ctrl+K | Search all commands by name, alias or description |

**Phones and tablets** (DESIGN.md §11): the tool sheet groups tools by
the families above (Draw, Annotate, Modify, Layers, Measure), and the
search box (the same as Ctrl+K) accepts names and aliases, so typed habits
carry over. The aid chips are GRID, SNAP and ORTHO. Long-press opens the
context menu. A tablet with a keyboard takes every alias and key here.
