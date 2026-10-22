# Blackboard curriculum

**Product:** BlackCAD, web-hosted build (core 0.6.0)
**Updated:** 2026-10-15
**Positioning:** "School for drafting, school for shop." BlackCAD is a real drafting tool, and the Blackboard beside it teaches the rules behind a good drawing. The educational side is the niche; the tool has to be useful on its own.

## How the board is organised

The board works like a school, so new material has an obvious place to go:

- **Subjects** are the tabs chalked along the top of the board.
- **Units** group lessons inside a subject.
- **Lessons** answer one question someone asks at the desk.
- **Steps** each hold one idea: a live chalk figure, a few sentences, the numbers that matter, and either a live rule check (Shop) or an exercise drawn in BlackCAD and checked by the board (Drafting).

Rules that apply everywhere:

1. **Figures are drawn from live numbers**, so a figure can't disagree with the text beside it.
2. **Shop thresholds live in Shop settings** and show their basis. Defaults are starting points a shop replaces with its own numbers.
3. **Drafting lessons teach BlackCAD as it is.** They name its commands, aliases, keys, panels and layers, read them from the program where they can, and teach every tool a drafter normally uses, each built in the core.

## Subjects (tab order)

| Tab | Status | Scope |
|---|---|---|
| **Drafting** | Taught now (13 lessons, 46 steps, 9 checked exercises) | BlackCAD's tools, then the language of drawings: layers, lines, units, scale, views, sections, dimensions, the sheet |
| **Shop** | Taught now (15 lessons, 56 steps) | Cut, bend and weld: fabrication rules a designer must draw to |
| **Electronics** | Syllabus shown, next term; not being built yet | Hardware around the board: outlines and mounting, panel cutouts, standoffs, vents, heat sinks, glands, grounding |
| Machining | Outline kept in code, tab hidden | Mill and lathe from the designer's side |

Drafting comes first because every other subject depends on it, and it's where a true beginner starts. Keep the tab count small: a new trade becomes a unit inside an existing subject before it earns its own tab. **Candidate later subject:** Building (architectural and construction drafting), once Drafting is reviewed.

## Drafting: what's built

| Unit | Lesson | Steps | Exercise the board checks | Show me / Try it |
|---|---|---|---|---|
| Start here: the tools | The command line | 3 | Ø40 circle at 0,0 (Ø1.5 in) | Draws it |
| | Points by the numbers | 5 | 100 × 50 plate from 0,0 with LINE and @ coordinates (4 × 2 in) | Draws it |
| | Snaps, Ortho and Polar | 4 | Line from a plate's top-edge midpoint to a hole's centre, exact | Set-up shapes; answer |
| | Select, grip and change | 5 | Mirror a bracket across a centre line, keep the original | Set-up shapes; answer |
| Lines that talk | Layers mean something | 4 | Plate and hole on VISIBLE, two centre lines on CENTER through the hole and past its edge | Answer |
| | Hidden lines and centre lines | 4 | Hidden edges on HIDDEN and a centre line on CENTER in a front view | Set-up views; answer |
| Size and scale | Units and precision | 3 | A 1-1/2" line typed in a metric drawing (50mm in an inch drawing) | Answer |
| | Draw full size | 2 | | Sheet border and title block around the drawing, scaled to fit |
| Views | Orthographic views | 5 | Top view in line with a front view, above (third angle) or below (first angle) | Set-up view; answer |
| | Sections | 3 | | |
| Dimensions | Dimensioning in BlackCAD | 4 | Four linear dimensions and a diameter on a plate, all on DIMENSION | Set-up plate; dimensioned answer |
| | Dimensioning rules | 4 | | Plate dimensioned from one datum |
| The sheet | Title block, notes and revisions | 4 | | Sheet border and title block |

**Exercises.** "Your turn" boxes give the exact keys to type in the drawing's own units. Set up places the starting shapes beside the drawing and fills the instructions with real coordinates. Check my work reads the open drawing (sizes, positions, layers, exact snapped points, counts of dimensions) and says what's right and what to fix. Show me draws a finished answer. Everything placed is one Undo step.

**Built from the program, not copied into it.** Alias lists come from BlackCAD's command list. The layer figure reads the open drawing's layers. Snap markers are drawn in the same shapes the canvas uses. Unit examples were checked against the core's own length parser.

## Tools the lessons teach (the gap list is closed)

Every tool the first Drafting draft had to work around is now built in the Rust core (`cad_core`) and reached through one session command line; the web page only prompts, previews and sends the line. The gap flags were removed with them, so no step says "not in BlackCAD yet".

| Tool | Commands | Taught in |
|---|---|---|
| Offset, Trim and Extend, Fillet and Chamfer, Scale and Stretch | `O`, `TR`/`T`, `EX`, `F`, `CHA`, `SC`, `S` | Offset, trim and fillet (new): an exercise each for offset, trim, fillet and scale |
| Array (rectangular, polar) | `AR` | Arrays: rows and circles of holes (new): a grid and a bolt circle; spacing check |
| Blocks and attributes | `B`, `I`, `ATT`, `ATE`, `X` | Blocks and attributes (new): make, insert, fill-in fields |
| Construction lines | `XL`, `RAY` | Orthographic views: draw the vertical through the front view's edge |
| Hatch | `H` | Sections: hatch a plate and leave the hole clear |
| Angular, baseline and continued dimensions; leaders | `DAN`, `DBA`, `DCO`, `LE`, `MLD` | Dimensioning in BlackCAD: dimension three holes from one edge; Dimensioning rules |
| Multiline text | `MT` | Title block, notes and revisions |
| Units, linetypes, lineweights, layer plot flag | `UN`, `LT`, `LTS`, `LW`; layer menu | Units and precision: set the decimals; Layers mean something |
| Layouts, viewports, page setup, PDF plot | `LO`, `MV`, `PAGE`, `PLOT`, `PRINT` | Draw full size: Try it makes a whole sheet in the core; Title block lesson |

Drafting now has 16 lessons in 7 units (the new unit is **Shape it**). Twenty-two exercises are checked in the browser in both metric and inch drawings.

## Shop: what's built

Unchanged from the first release: 15 lessons in Forming, Cutting, Drawings for the shop, and Weld and assembly, plus Shop settings, Check my drawing and the printable shop card. See `LESSON-SOURCES.md` for sources and the sign-off column.

**Commands:** `BOARD` (F1), `SHOPCHECK` (`CHECK`, `CHK`), `SHOPSET` (`SHOP`).

## Next terms

### Electronics (planned, not being built yet)

1. Board outlines and mounting holes.
2. Panel cutouts for connectors, switches and displays, with the tolerance stack from board to panel.
3. Standoffs and stack heights.
4. Vent slots and airflow (reuses the Cutting rules for web and smallest hole).
5. Heat sinks and thermal paths.
6. Glands, grommets and strain relief; grounding studs.

### Machining (outline only, tab hidden)

Inside corners need a radius; pockets and depth; drill points and blind-hole depth; tap drills and thread depth; shoulders, grooves and undercuts; chamfers and edge breaks; fits and limits; surface finish; cutting speed for the designer. The owner can review these from machinist experience; no CAM content.

## Engineering notes for the next builds

- **Lesson packs as data.** With 28 lessons, lesson text, steps and exercise instructions should move to plain data files; figures and checks stay as code, referenced by name.
- **More drafting checks.** The exercise checks already read sizes, layers and exact points. The same reader can power a general "check my drawing" for drafting: objects on layer 0, text off NOTES, dimensions off DIMENSION, centre lines missing on holes.
- **Shop settings export and import**, so a teacher can hand a class one set of numbers.
- **Web vs native.** The hosted build is the same app. The only differences should be the browser's limits: one thread, the core's 1 GiB memory ceiling, file access through the browser's pickers, and no `.cdl` lock file.
