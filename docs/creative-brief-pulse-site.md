# Art Direction Brief: Pulse showcase site

## Project: Pulse — one-page site

Owner: mercy123xz · Built by: Claude · Format: mixed (product screenshots + CSS-drawn graphics, no photography or illustration commissioned)

## 1. Project context

Pulse is a free, open-source, native Windows music player written in Rust (v0.3.0). It needs a single page that explains what it is in five seconds and gets visitors to the installer. Distribution: GitHub Pages, linked from the repo README and shared by the owner. No budget, same-day turnaround.

## 2. Concept

**Premise:** A music player that weighs almost nothing and still feels like it was made for you.

**Emotional through-line:** relief ("finally, something light") turning into ownership ("and it's mine to shape": themes, playlists, profile).

**Role of the brand:** the violet Pulse mark and gradient are the only loud thing on the page. The app's screenshots carry the story; the page frames them like a gallery wall.

**Takeaway:** "It's free, it's small, it plays anything. Download it."

## 3. Audience

Windows users who listen to music daily, many of them young and comfortable with tech, who are curious about a lighter alternative to big streaming apps. They skim. They trust screenshots more than adjectives.

## 4. Subjects

The five owner-supplied screenshots, in order of strength:

1. Charts / Top songs table: the hero. It shows the app's density and polish at a glance.
2. Popular albums grid: proof of a rich catalog, colorful.
3. Popular artists row: round portraits, the warmest frame.
4. Popular playlists row: color and variety.
5. Home "Discover" with genre chips: navigation and breadth.

Screenshots show real album art and artists. That's acceptable as product screenshots of the owner's own app; no artwork is cropped out and used on its own.

## 5. Composition

- Hero: left-aligned headline and download button; the charts screenshot sits in a CSS-drawn window frame that bleeds off the right edge at large widths and stacks below the text on mobile.
- Feature sections alternate text and screenshot. Wide screenshots (artists, playlists) run as full-width "strips" with a soft fade at the edges.
- Theme section: four CSS-drawn mini app windows (no images), one per built-in theme, using each theme's real color values.

## 6. Lighting

Not photographic. "Light" comes from one soft violet glow behind the hero window: a radial gradient, no blur filters, which are expensive to paint.

## 7. Color and treatment

- Page: near-black `#0b0b0e` (the app's own background), surfaces `#16161b`, text `#ededf1`, secondary `#9696a4` (all from the app's WCAG-checked palette).
- Accent: Pulse violet `#7c3aed` with `#a78bfa` for text and icons. Accent gradient only on the logo, the primary button and the hero glow.
- Screenshots shown untreated (true color), with a 1 px border and large rounded corners so they read as app windows.

## 8. Tone

Confident and plain. Short sentences, no hype words ("revolutionary", "blazing"), no comparisons that name other services. Every claim on the page is something the app actually does today.

## 9. Reference moodboard

Direction only, not imitation: the restraint of developer-tool landing pages (one accent, dark canvas, product-first), the product-shot framing of desktop-app download pages. No competitor music services used as references.

## 10. What to avoid

- Stock photos of people with headphones; 3D blobs; glassmorphism; neon gradients everywhere.
- Other services' names or logos in headlines.
- Claims about speed numbers that weren't measured, or "100% legal" style promises.
- Web fonts and JavaScript the page doesn't need.

## 11. Deliverables

`site/index.html` (single file, inline CSS), `site/img/*` (screenshots as AVIF + WebP at 1× and 2× widths with exact dimensions), favicon from the app icon, social preview image (1200×630), and a GitHub Pages workflow.

Variants by viewport: ≥1100 px two-column hero with bleed; 700–1100 px narrower bleed; <700 px stacked, screenshots full width, horizontal-scroll strips for the wide shots.

## 12. Standards

- Performance: LCP < 2.5 s on mobile Lighthouse, CLS < 0.1, total page weight under ~1 MB on desktop, no render-blocking requests beyond the HTML.
- Accessibility: text contrast ≥ 4.5:1, every screenshot with real alt text, visible focus, works without JavaScript.
- Approved looks like: the screenshots are the star and nothing on the page fights them. Not approved: decoration that competes with them.

## 13. Approvals

Owner reviews the live page once; one revision round expected.

## 15. Open questions

None blocking. A full-window screenshot (with sidebar and player bar) would make a stronger hero later.
