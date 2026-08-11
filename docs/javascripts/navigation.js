(() => {
  const localeLabels = new Map([
    ["English", "en"],
    ["日本語", "ja"],
    ["简体中文", "zh"],
  ]);

  const currentLocale = () => {
    const firstSegment = window.location.pathname.split("/").filter(Boolean)[0];

    if (firstSegment === "ja-JP") return "ja";
    if (firstSegment === "zh-CN") return "zh";
    return "en";
  };

  const showCurrentLocale = () => {
    const locale = currentLocale();
    document.documentElement.lang =
      locale === "ja" ? "ja-JP" : locale === "zh" ? "zh-CN" : "en";
    const groups = document.querySelectorAll(
      ".md-sidebar--primary li.md-nav__item--nested",
    );

    for (const group of groups) {
      const header = group.querySelector(
        ":scope > .md-nav__container, :scope > label.md-nav__link",
      );
      const label = header?.querySelector(".md-ellipsis")?.textContent.trim();
      const groupLocale = localeLabels.get(label);

      if (groupLocale) {
        group.hidden = groupLocale !== locale;
        header.hidden = groupLocale === locale;
        header.style.display = groupLocale === locale ? "none" : "";
      }
    }
  };

  showCurrentLocale();

  if (typeof document$ !== "undefined") {
    document$.subscribe(showCurrentLocale);
  } else {
    document.addEventListener("DOMContentLoaded", showCurrentLocale, { once: true });
  }
})();
