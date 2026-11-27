// The Beginner's Guide, bundled into the app so it works offline.
// The chapters live in apps/admin-center/guide/ as Markdown, which GitHub also renders.

const RAW = import.meta.glob("../../guide/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const IMAGES = import.meta.glob("../../guide/images/*.png", { import: "default", eager: true }) as Record<string, string>;

export interface Chapter { slug: string; file: string; title: string; body: string; part: "home" | "ideas" | "doing" | "reference" }

const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

export const CHAPTERS: Chapter[] = Object.entries(RAW)
  .map(([path, body]) => {
    const file = base(path);
    const slug = file === "README.md" ? "" : file.replace(/\.md$/, "");
    const title = body.match(/^# (.+)$/m)?.[1]?.trim() ?? slug;
    const n = Number(slug.slice(0, 2));
    const part: Chapter["part"] = slug === "" ? "home" : slug === "glossary" ? "reference" : n >= 7 ? "doing" : "ideas";
    return { slug, file, title, body, part };
  })
  .sort((a, b) => (a.slug === "" ? -1 : b.slug === "" ? 1 : a.slug === "glossary" ? 1 : b.slug === "glossary" ? -1 : a.slug.localeCompare(b.slug)));

export const chapter = (slug: string) => CHAPTERS.find((c) => c.slug === slug);

/** "images/ous.png" in a chapter → the bundled asset URL. */
export const guideImage = (src: string) => IMAGES[`../../guide/${src.replace(/^\.\//, "")}`];

/** GitHub-style heading anchor: "Step 2: Create a token" → "step-2-create-a-token". */
export const headingId = (text: string) =>
  text.toLowerCase().replace(/[`*_]/g, "").replace(/[^\p{L}\p{N}\s-]/gu, "").trim().replace(/\s/g, "-");

/** In-app route for a guide chapter, optionally at a section. */
export const guidePath = (slug: string, section?: string) => `/guide${slug ? `/${slug}` : ""}${section ? `?s=${encodeURIComponent(section)}` : ""}`;

/** Chapter titles without the leading number, for "Guide: ..." links. */
export const shortTitle = (slug: string) => (chapter(slug)?.title ?? "Guide").replace(/^\d+\.\s*/, "").split(":")[0];
