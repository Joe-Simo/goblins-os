import type { MetadataRoute } from "next";

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    {
      url: "https://goblinsos.com",
      lastModified: new Date("2026-08-21T00:00:00Z"),
      changeFrequency: "weekly",
      priority: 1,
    },
  ];
}
