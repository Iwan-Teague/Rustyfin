## Design Context

### Users
Rustyfin is primarily used from a home-server context by friends and family. The core job is social media continuity: users should quickly resume what they were previously watching and immediately see available rooms (especially watch rooms and audio channels) they can join together. Secondary capabilities can stay powerful, but this core social-media flow should feel obvious and low-friction.

### Brand Personality
The intended personality is **cinematic, confident, technical**. The interface should feel polished and professional (inspired by Stripe and Apple), while still reading as a feature-rich web application in the spirit of Jellyfin and Discord. It should project capability without feeling cold or intimidating.

### Aesthetic Direction
Stay dark-mode-first and cinematic. Preserve the current Rustyfin accent gradient (**orange -> pink -> purple**) as a brand signature. Favor clean composition and clear hierarchy, avoiding layouts that feel cramped or clustered, but also avoid excessive empty space that weakens functional density. Reuse existing Rustyfin interaction patterns and shared motion helpers rather than introducing disconnected one-off effects.

### Design Principles
1. **Design for social continuation first**: prioritize resume-state visibility and room joinability over secondary controls.
2. **Keep the cinematic dark identity**: preserve the established dark palette and signature accent gradient.
3. **Balance density with clarity**: information should be scannable and efficient, never cramped and never sparse for its own sake.
4. **System consistency over novelty**: use shared button/delete interaction patterns and existing UI tokens as defaults.
5. **Accessibility as baseline quality**: meet WCAG 2.1 AA expectations and preserve clear contrast and readable hierarchy in all new UI work.
