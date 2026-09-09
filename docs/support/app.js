const walletAddress = "GYqhL927sKcsF86PzQEiYQVnmTScT6LesMohD5ugkXzN";
const copyButton = document.querySelector("#copy-address");
const toast = document.querySelector("#toast");
let toastTimer;

if (window.lucide) {
  window.lucide.createIcons();
}

function setCopyState(copied) {
  copyButton.querySelector("span").textContent = copied ? "Copied" : "Copy address";
  const icon = copyButton.querySelector("i, svg");
  if (icon && window.lucide) {
    icon.outerHTML = `<i data-lucide="${copied ? "check" : "copy"}" aria-hidden="true"></i>`;
    window.lucide.createIcons();
  }
}

copyButton.addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(walletAddress);
  } catch {
    const textArea = document.createElement("textarea");
    textArea.value = walletAddress;
    textArea.style.position = "fixed";
    textArea.style.opacity = "0";
    document.body.appendChild(textArea);
    textArea.select();
    document.execCommand("copy");
    textArea.remove();
  }

  setCopyState(true);

  toast.classList.add("visible");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.classList.remove("visible");
    setCopyState(false);
  }, 2200);
});
