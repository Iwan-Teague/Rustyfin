# Commercial Sale Legal Risk Outline (Rustyfin)

Date: 2026-02-26  
Scope: Selling Rustyfin bundled with hardware as a private home-server product.

This document is a practical risk outline, not legal advice. Have counsel review before commercial launch.

## Executive Summary

The main blockers are not Rust/Next.js/Postgres licensing. The high-risk items are platform/content terms (YouTube/TMDB), media rights/compliance, and operational licensing/notice obligations.

## 1) YouTube Download/Extraction Risk

### Concern
The product includes functionality that downloads or extracts audio from YouTube content for Listen Together workflows.

### Potential Legal Ramifications
- Breach of YouTube Terms/Developer Policies.
- Forced takedown of the feature or service endpoints.
- Account/key/API suspension.
- Potential legal claims for circumvention or unauthorized access/use.
- Increased payment processor/platform risk if service is reported for policy abuse.

### Risk Level
High (commercial context).

## 2) TMDB Commercial Use and Attribution Risk

### Concern
TMDB API usage is often non-commercial by default unless covered by specific commercial terms; attribution obligations apply.

### Potential Legal Ramifications
- Loss of API access.
- Contract/compliance claims for unauthorized commercial use.
- Need to remove metadata/artwork features under short notice.
- Brand/attribution non-compliance exposure.

### Risk Level
High (if commercial license/terms are not in place).

## 3) FFmpeg/Codec Patent Distribution Risk

### Concern
Commercial distribution of media processing can trigger codec patent/licensing obligations (jurisdiction dependent), even when OSS licensing is handled.

### Potential Legal Ramifications
- Patent licensing claims in affected territories.
- Forced feature limitation or codec removal in certain markets.
- Product launch delays pending legal/codec review.

### Risk Level
Medium to High (depends on enabled codecs and jurisdictions).

## 4) Open-Source License Compliance Packaging Risk

### Concern
Commercial distribution requires complete OSS notices and compliance handling for all bundled dependencies (including LGPL/MPL components in transitive tree).

### Potential Legal Ramifications
- License breach claims or cure demands.
- Reputational and contractual risk with customers/partners.
- Forced remediation release and legal overhead.

### Risk Level
Medium.

## 5) Docker Desktop / Tooling Subscription Risk (Business Use)

### Concern
Docker Desktop licensing may require paid subscriptions depending on org size/revenue/use case.

### Potential Legal Ramifications
- Subscription non-compliance for internal commercial operations.
- Audit/payment exposure.

### Risk Level
Low to Medium (easy to mitigate, but must be tracked).

## 6) Content Rights and Privacy/Transcription Compliance Risk

### Concern
Streaming, redistribution, and transcription features create rights and consent obligations.

### Potential Legal Ramifications
- Copyright claims for unauthorized content usage.
- Privacy/data-protection claims (consent, recording/transcription notice, retention controls).
- Regional regulatory exposure if transcript retention and consent are not explicit.

### Risk Level
High (especially where users upload/share/record content).

## Pre-Sale Mitigation Checklist

1. Remove or hard-disable YouTube download/extraction in commercial builds unless fully licensed and policy-compliant.
2. Obtain TMDB commercial approval (if required) and implement mandatory attribution everywhere metadata/artwork appears.
3. Run a codec legal review for target sales regions; document permitted codec set per region.
4. Ship a full third-party notices bundle and license inventory with each release.
5. Confirm Docker tooling subscription posture for business use, or use compliant alternatives in build/deploy pipeline.
6. Add explicit consent + policy UX for transcription/recording, with retention controls and admin audit trails.

## Go/No-Go Recommendation

- **No-Go** for broad commercial sale until items 1, 2, and 6 are remediated.
- **Conditional Go** after legal review and completion of the checklist above.

## Reference Links

- YouTube Terms: https://www.youtube.com/t/terms
- YouTube Developer Policies: https://developers.google.com/youtube/terms/developer-policies
- TMDB FAQ/Developer Docs: https://developer.themoviedb.org/docs/faq
- FFmpeg Legal: https://ffmpeg.org/legal.html
- Docker Desktop License: https://docs.docker.com/subscription/desktop-license/
