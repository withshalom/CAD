# BlackCAD Advisory — Shop-themed Blackboard lessons

**Subject:** BlackCAD 0.5.1, the Blackboard coach (F1 / `BOARD` / `BB`)
**Date:** 2026-10-09
**Nature:** Advisory only. No build.
**Clean-room note:** Everything here comes from observing BlackCAD's own behaviour and from general, publicly known shop practice. No other project's code, content or behaviour is used. Write every lesson in your own words and your own drawings, and take every number from a public handbook, a supplier datasheet, or your own measured shop data. Keep a one-line source note per lesson (see Provenance).

---

## 1. What exists today

Two lessons, five steps each, both sheet-metal: *Bend allowance* and *Holes, slots and reliefs*. Each step is a chalk figure, a short explanation, and sliders (T, R, K, angle, legs, hole distance, relief size) whose values drive both the formulas and the figure. Verified: for T=3, R=3, K=0.4, 90°, legs 50 and 30, the board shows OSSB 6, BA 6.60 and flat length 74.60 mm, matching the standard neutral-axis formulas. The units switch (mm/in) and a laser/plasma process switch already exist.

**The strength to protect:** every figure is drawn from the same numbers the formulas use. Keep that rule for all new lessons: no static pictures that can disagree with the maths.

## 2. Design principles for shop lessons

1. **One decision per lesson.** Each lesson answers a question a designer asks at the desk ("how far can this hole sit from the edge?"), not a topic.
2. **Numbers you can move, rules you can state.** Show the rule as a sentence and as a live check ("Clear / Too close: 6.0 of 8.5 needed"), as the hole-to-bend step already does.
3. **Process-aware.** Laser and plasma give different answers. Use the existing process switch everywhere it matters; extend it to other processes only when you have data for them.
4. **Starting points, not laws.** Shop rules depend on machine, gas, nozzle, material and die. Label every threshold as a default the shop can override, and say what it depends on.
5. **Chalk voice.** Short sentences, one idea per step, a worked example before the formula.
6. **Optional and quiet.** Never auto-open after the first run; remember where the user left off (already done via local storage).

## 3. Lesson roadmap

Order is by value to a designer who prepares drawings for thermal cutting and press-brake forming. Each entry: question → what the figure shows → sliders → rule shown.

### Tier 1: extend what's there (sheet metal)
1. **Minimum flange and die choice.** *Can this flange be formed?* Figure: flange on a V-die. Sliders: T, flange length, die opening. Rule: shortest flange is tied to die width, which is a multiple of thickness; show the die opening as the shop's own setting.
2. **Bend deduction and setback table.** *What number do I give the brake operator?* Same figure language as Bend allowance; adds BD and a printable table for the user's own K-factors.
3. **Grain direction and bend cracking.** *Which way should this part sit on the sheet?* Figure: sheet with grain arrows, bend lines at 0°/90°. Rule: bends across the grain tolerate tighter radii; show min radius as a multiple of T chosen from the material the user picks.
4. **Corner reliefs and bend-to-bend.** *Where do two bends meet?* Builds on the existing relief step; adds jog/Z-bend minimum step height.

### Tier 2: thermal cutting (laser / plasma)
5. **Kerf and what it does to your dimensions.** *Is the hole I drew the hole I'll get?* Figure: cut path offset to the inside of holes and outside of parts. Sliders: kerf width, thickness. Rule: holes come out kerf-width small unless the path is compensated; show both sides.
6. **Smallest hole and narrowest web.** *How small can I cut?* Figure: hole diameter vs thickness, web between cuts. Rule: minimum hole is a ratio of thickness and differs by process (laser tighter than plasma); webs narrower than a stated multiple of thickness warp or burn through. Wire the web check to the existing `g` (web between cuts) slider.
7. **Lead-ins, lead-outs and pierce points.** *Where does the cut start?* Figure: pierce outside the part with a lead-in arc into the contour. Rule: pierce in scrap, not on the edge you keep; lead-in length scales with thickness and process.
8. **Heat and thin features.** *Why did this tab curl?* Figure: long thin tabs and tight clusters of small holes with a heat-build indicator. Rule of thumb only; present as a warning, not a calculation.
9. **Edge quality and bevel.** *Which face is the good face?* Figure: cut-edge taper and kerf shape in plasma vs laser; where the taper lands relative to hole vs part.

### Tier 3: drawing practice (CAD, not CAM)
10. **Dimensioning for fabrication.** *What does the shop actually need?* Baseline vs chain dimensions, tolerance stack-up over a hole pattern, dimensions to features that can be measured. Fits the existing green dimension layer.
11. **Layers and linetypes as communication.** *What do VISIBLE, CENTER, HIDDEN and DIMENSION mean to the person reading my drawing?* Uses BlackCAD's own default layers. Note: keep this CAD-only; do not teach machine-operation layers (consistent with the "CAD, not CAM" rule).
12. **Units and scale.** *mm vs inch without mistakes.* Fractions, decimal places and how the file stores units; ties to the existing unit switch.
13. **Reading a tolerance block.** Title-block tolerances, general vs specific, and what a laser or plasma shop can reasonably hold.

### Tier 4: weld and assembly basics (later)
14. **Fit-up and gap.** Plate edge prep and root gap relative to thickness.
15. **Distortion and sequence.** Why part symmetry and tab-and-slot features help assembly.

## 4. Features around the lessons

| Feature | Value | Note |
|---|---|---|
| **Check my drawing** button on a lesson step | Highest. Turns a lesson into a design-rule check on the user's real geometry ("this hole is 5.0 from a bend; the rule needs 8.5") | Needs the core to measure distances from entity to entity and to know which lines are bends. Decide how bends are marked (layer, linetype or a property) and say so in the lesson. Start with hole-to-edge and hole-to-hole, which need no bend metadata. |
| **Shop settings page** (kerf, min-hole ratio, die opening, K-factor, grain) | High | One place for the user's own numbers; every lesson reads from it; lets a shop replace defaults with measured values. |
| **Try it on the drawing** (draw the lesson's example) | High | Places the example geometry as real objects on a layer so the user can edit it. Use the existing command path so it lands in undo. |
| **Print / save as a one-page cheat sheet** | Medium | Gives shops something to pin up. SVG export already exists. |
| **Progress marks** | Low | Local only; checkmark per lesson. Keep it unobtrusive. |
| **Lesson packs** (data, not code) | Medium, later | Lessons as plain data files (title, steps, sliders, formulas) so new lessons and shop-specific ones don't need a rebuild. Design the format yourself; do not borrow another product's lesson or tutorial format. |

## 5. Content and accuracy rules

- Every threshold shows its **basis** in small chalk text ("typical starting point; depends on gas, nozzle and machine").
- Every formula shows its **inputs** and the **steps** in the order a person would calculate by hand.
- Provide a **worked example** with round numbers before the sliders.
- Defaults need an **override path**; never let a lesson present a default as a rule of the shop.
- Avoid brand-specific claims (machine models, software names). Where a standard is cited, cite the public standard by name and edition.
- Have a working fabricator read each lesson before release; one sentence of "this didn't match our shop" feedback is worth more than any source.

## 6. Provenance (protecting clean-room status)

- Keep a `LESSON-SOURCES.md` in the project with one row per lesson: question, formulas used, public source(s) or "own measurement", author, date.
- Write lesson text, figure geometry and sample drawings fresh. Do not paste from other CAD or CAM tutorials, vendor manuals, or other software's help.
- Measurements from your own shop count as your own data; record the machine and material.
- If a lesson's content came from a conversation with a tool or person, note that and still verify against a public source before shipping.

## 7. Interface notes from testing

- **Phone:** the board covers the full drawing, and text says "the drawing on the left" while the figure sits above. Reword to be layout-neutral ("the figure") and consider a half-height board so the real drawing stays visible.
- **Desktop:** the lesson list is clear; add a visible "step n of 5" and Back/Next so the 5-step flow is obvious at a glance.
- **Accessibility:** chalk text on near-black is readable but thin on small screens; offer a high-contrast toggle and keep body text at the size used in the panel.
- **Performance:** none observed; lessons are light.

## 8. Suggested order of work

1. Fix the phone wording and layout; add step n of 5 and Back/Next.
2. Add the **Shop settings** page and read the two existing lessons from it.
3. Build **Kerf**, **Smallest hole and narrowest web**, and **Lead-ins** (Tier 2). These match the shop you design for and use only thickness, kerf and process.
4. Add **Check my drawing** for hole-to-edge and hole-to-hole.
5. Add Tier 1 lessons, then Tier 3.
6. Move lessons to data files when you have about six.

## 9. Acceptance checks

- Every figure changes when its slider changes, and every number on the board matches a hand calculation for at least two worked examples.
- Every threshold has a visible basis and can be overridden in Shop settings.
- A fabricator reviews each lesson and signs off in `LESSON-SOURCES.md`.
- Phone and desktop layouts both show the figure, text and sliders without covering the active step.
- No lesson mentions machine-operation layers or CAM concepts beyond what a designer must know to draw correctly.
