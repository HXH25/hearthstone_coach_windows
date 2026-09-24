(() => {
  const reveal = document.querySelectorAll('.reveal');
  if (!('IntersectionObserver' in window)) {
    reveal.forEach(el => el.classList.add('visible'));
    return;
  }
  const observer = new IntersectionObserver(entries => {
    entries.forEach(entry => {
      if (entry.isIntersecting) {
        entry.target.classList.add('visible');
        observer.unobserve(entry.target);
      }
    });
  }, { threshold: 0.08, rootMargin: '0px 0px -28px' });
  reveal.forEach(el => observer.observe(el));
})();
