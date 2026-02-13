# Theme, Styling, and Animation Spec (Extreme Expansion)

## 1. Goals
- dark, cinematic, art-forward
- motion explains state; never distracts
- smooth on mid-range phones

## 2. Tokens via CSS variables
```css
:root {
  --bg: 10 12 16;
  --fg: 230 232 236;
  --muted: 150 156 168;

  --surface-1: 16 18 24;
  --surface-2: 22 24 32;
  --surface-3: 30 32 44;

  --primary: 120 180 255;
  --danger: 255 90 90;
  --success: 120 220 160;
  --warning: 255 190 90;

  --r-sm: 10px;
  --r-md: 14px;
  --r-lg: 18px;

  --shadow-1: 0 6px 18px rgb(0 0 0 / 0.25);

  --dur-fast: 120ms;
  --dur-med: 200ms;
  --dur-slow: 320ms;

  --ease-out: cubic-bezier(.2,.8,.2,1);
  --ease-in: cubic-bezier(.4,0,1,1);
  --ease-inout: cubic-bezier(.4,0,.2,1);
}
```

WCAG contrast explanation:
https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum

## 3. Motion system
Material motion overview:
https://m3.material.io/styles/motion/overview/specs

Reduced motion:
- MDN: https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion
- W3C technique: https://www.w3.org/WAI/WCAG21/Techniques/css/C39

```css
@media (prefers-reduced-motion: reduce) {
  :root { --dur-fast: 0.001ms; --dur-med: 0.001ms; --dur-slow: 0.001ms; }
  * { scroll-behavior: auto !important; }
}
```

Performance guidance:
- web.dev: https://web.dev/articles/animations-guide
- MDN: https://developer.mozilla.org/en-US/docs/Web/Performance/Guides/CSS_JavaScript_animation_performance

## 4. Components
Poster tiles:
- 2xl radius, subtle shadow, hover scale (desktop), focus ring (TV/keyboard)

Backdrop headers:
- crossfade between backdrops with gradient overlay for legibility

Skeleton loaders:
- shimmer, but simplified/disabled in reduced motion
