// Applies the saved colour theme before the first paint (external file: the CSP forbids inline scripts).
;(function () {
  var theme = 'dark'
  try {
    if (localStorage.getItem('theme') === 'light') theme = 'light'
  } catch (e) {
    /* storage unavailable */
  }
  var root = document.documentElement
  root.classList.toggle('dark', theme === 'dark')
  root.dataset.theme = theme
  root.style.colorScheme = theme
})()
