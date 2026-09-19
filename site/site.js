/* QuotaBar: progressive enhancements; downloads and content work without JS. */
"use strict";

document.querySelectorAll("[data-cmd]").forEach((command) => {
  const button = command.querySelector("button");
  const code = command.querySelector("code");
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(code.textContent.trim());
      button.textContent = "Copied";
    } catch {
      // Keep the entire command selectable on restricted origins.
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(code);
      selection.removeAllRanges();
      selection.addRange(range);
      button.textContent = "Select & copy";
    }
    setTimeout(() => { button.textContent = "Copy"; }, 2500);
  });
});

if ("IntersectionObserver" in window) {
  const links = [...document.querySelectorAll(".rail a")];
  const observer = new IntersectionObserver((entries) => {
    const active = entries.find((entry) => entry.isIntersecting);
    if (!active) return;
    links.forEach((link) => {
      const current = link.hash === "#" + active.target.id;
      link.classList.toggle("on", current);
      if (current) link.setAttribute("aria-current", "location");
      else link.removeAttribute("aria-current");
    });
  }, { rootMargin: "-20% 0px -50% 0px" });
  document.querySelectorAll("main > section[id]").forEach((section) => observer.observe(section));
}
