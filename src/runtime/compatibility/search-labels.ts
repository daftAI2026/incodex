/** Official Search aria-labels, including Traditional Chinese. */
export const SEARCH_LABELS = new Set([
  "Search",
  "搜索",
  "搜尋",
  "搜寻",
  "検索",
  "검색",
  "Rechercher",
  "Suche",
  "Buscar",
  "Cerca",
  "Pesquisar",
  "Procurar",
  "Поиск",
  "Пошук",
  "Szukaj",
  "Hledat",
  "Hľadať",
  "Keresés",
  "Căutare",
  "Ara",
  "Søk",
  "Sök",
  "Søg",
  "Zoeken",
  "Hae",
  "Αναζήτηση",
  "חיפוש",
  "بحث",
  "खोजें",
  "खोज",
  "ค้นหา",
  "Tìm kiếm",
  "Cari",
  "Pencarian",
]);

export function isSearchLabel(label: string | null | undefined): boolean {
  const value = (label || "").trim();
  if (!value) return false;
  if (SEARCH_LABELS.has(value)) return true;
  const lower = value.toLowerCase();
  return lower === "search" || lower.startsWith("search ");
}
