import {
  AppWindow,
  Archive,
  Code2,
  FileQuestion,
  FileSpreadsheet,
  FileText,
  Film,
  Image,
  LifeBuoy,
  Music,
  Palette,
  Presentation,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { Category } from "@/types";

export interface CategoryMeta {
  id: Category;
  label: string;
  icon: LucideIcon;
  description: string;
}

/** Display order matches the backend's category list. */
export const CATEGORIES: CategoryMeta[] = [
  {
    id: "documents",
    label: "Documents",
    icon: FileText,
    description: "PDFs, text and word-processor files",
  },
  { id: "images", label: "Images", icon: Image, description: "Photos, screenshots and graphics" },
  { id: "videos", label: "Videos", icon: Film, description: "Movies, clips and recordings" },
  { id: "audio", label: "Audio", icon: Music, description: "Music, podcasts and voice memos" },
  {
    id: "archives",
    label: "Archives",
    icon: Archive,
    description: "Compressed files and disk images",
  },
  {
    id: "installers",
    label: "Installers",
    icon: AppWindow,
    description: "Application installers and packages",
  },
  {
    id: "development",
    label: "Development",
    icon: Wrench,
    description: "Configs, build output, databases",
  },
  { id: "code", label: "Code", icon: Code2, description: "Source code and scripts" },
  {
    id: "spreadsheets",
    label: "Spreadsheets",
    icon: FileSpreadsheet,
    description: "Workbooks and CSV data",
  },
  { id: "presentations", label: "Presentations", icon: Presentation, description: "Slide decks" },
  { id: "design", label: "Design", icon: Palette, description: "Design, 3D and CAD files" },
  {
    id: "backups",
    label: "Backups",
    icon: LifeBuoy,
    description: "Backup copies and old versions",
  },
  { id: "other", label: "Other", icon: FileQuestion, description: "Unrecognized formats" },
];

const BY_ID = new Map(CATEGORIES.map((c) => [c.id, c]));

export function categoryMeta(id: string): CategoryMeta {
  return BY_ID.get(id as Category) ?? CATEGORIES[CATEGORIES.length - 1]!;
}
