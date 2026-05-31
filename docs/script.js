/* ═══════════════════════════════════════════════════════════════
   FileDrop — Interactive Scripts
   Background particles · Scroll reveals · Tabs · Nav toggle
   ═══════════════════════════════════════════════════════════════ */

document.addEventListener('DOMContentLoaded', () => {

  /* ── Background Particle Canvas ── */
  const canvas = document.getElementById('bg-canvas');
  if (canvas) {
    const ctx = canvas.getContext('2d');
    let w, h;
    const particles = [];
    const PARTICLE_COUNT = 80;

    function resize() {
      w = canvas.width  = window.innerWidth;
      h = canvas.height = window.innerHeight;
    }
    resize();
    window.addEventListener('resize', resize);

    class Particle {
      constructor() { this.reset(); }
      reset() {
        this.x  = Math.random() * w;
        this.y  = Math.random() * h;
        this.vx = (Math.random() - 0.5) * 0.4;
        this.vy = (Math.random() - 0.5) * 0.4;
        this.r  = Math.random() * 2 + 0.5;
        this.alpha = Math.random() * 0.4 + 0.1;
      }
      update() {
        this.x += this.vx;
        this.y += this.vy;
        if (this.x < 0 || this.x > w) this.vx *= -1;
        if (this.y < 0 || this.y > h) this.vy *= -1;
      }
      draw() {
        ctx.beginPath();
        ctx.arc(this.x, this.y, this.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(0,232,123,${this.alpha})`;
        ctx.fill();
      }
    }

    for (let i = 0; i < PARTICLE_COUNT; i++) {
      particles.push(new Particle());
    }

    function connectParticles() {
      for (let i = 0; i < particles.length; i++) {
        for (let j = i + 1; j < particles.length; j++) {
          const dx = particles[i].x - particles[j].x;
          const dy = particles[i].y - particles[j].y;
          const dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < 150) {
            const alpha = (1 - dist / 150) * 0.08;
            ctx.beginPath();
            ctx.moveTo(particles[i].x, particles[i].y);
            ctx.lineTo(particles[j].x, particles[j].y);
            ctx.strokeStyle = `rgba(0,232,123,${alpha})`;
            ctx.lineWidth = 0.5;
            ctx.stroke();
          }
        }
      }
    }

    function animate() {
      ctx.clearRect(0, 0, w, h);
      particles.forEach(p => { p.update(); p.draw(); });
      connectParticles();
      requestAnimationFrame(animate);
    }
    animate();
  }


  /* ── Navbar scroll effect ── */
  const navbar = document.querySelector('.navbar');
  window.addEventListener('scroll', () => {
    if (window.scrollY > 40) {
      navbar.classList.add('scrolled');
    } else {
      navbar.classList.remove('scrolled');
    }
  });


  /* ── Mobile nav toggle ── */
  const navToggle = document.querySelector('.nav-toggle');
  const navLinks  = document.querySelector('.nav-links');
  if (navToggle) {
    navToggle.addEventListener('click', () => {
      navLinks.classList.toggle('open');
    });
    navLinks.querySelectorAll('a').forEach(link => {
      link.addEventListener('click', () => navLinks.classList.remove('open'));
    });
  }


  /* ── Scroll reveal ── */
  const reveals = document.querySelectorAll('.reveal, .reveal-children');
  const revealObserver = new IntersectionObserver((entries) => {
    entries.forEach(entry => {
      if (entry.isIntersecting) {
        entry.target.classList.add('visible');
      }
    });
  }, { threshold: 0.12 });
  reveals.forEach(el => revealObserver.observe(el));


  /* ── Tab switching (Instructions) ── */
  const tabBtns   = document.querySelectorAll('.tab-btn');
  const tabPanels = document.querySelectorAll('.tab-panel');

  tabBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      const target = btn.dataset.tab;

      tabBtns.forEach(b => b.classList.remove('active'));
      tabPanels.forEach(p => p.classList.remove('active'));

      btn.classList.add('active');
      document.getElementById(target).classList.add('active');
    });
  });


  /* ── Copy-to-clipboard ── */
  document.querySelectorAll('.copy-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const codeBlock = btn.parentElement;
      const code = codeBlock.querySelector('code')?.textContent || codeBlock.textContent;
      navigator.clipboard.writeText(code.replace('Copy', '').trim()).then(() => {
        const original = btn.textContent;
        btn.textContent = 'Copied!';
        setTimeout(() => { btn.textContent = original; }, 1500);
      });
    });
  });


  /* ── Smooth anchor scroll ── */
  document.querySelectorAll('a[href^="#"]').forEach(anchor => {
    anchor.addEventListener('click', e => {
      e.preventDefault();
      const target = document.querySelector(anchor.getAttribute('href'));
      if (target) {
        target.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
    });
  });


  /* ── Auto-detect OS and highlight download card ── */
  const ua = navigator.userAgent.toLowerCase();
  let detectedOS = 'linux';
  if (ua.includes('win'))   detectedOS = 'windows';
  if (ua.includes('mac'))   detectedOS = 'macos';

  const detectedCard = document.querySelector(`.download-card[data-os="${detectedOS}"]`);
  if (detectedCard) {
    detectedCard.style.borderColor = 'rgba(0,232,123,0.35)';
    detectedCard.style.boxShadow  = '0 0 40px rgba(0,232,123,0.12)';
    const badge = document.createElement('div');
    badge.textContent = '✦ Recommended for your system';
    badge.style.cssText = `
      font-size: 0.75rem;
      font-weight: 600;
      color: #00e87b;
      background: rgba(0,232,123,0.08);
      border: 1px solid rgba(0,232,123,0.2);
      border-radius: 99px;
      padding: 4px 14px;
      margin-bottom: 16px;
      display: inline-block;
      font-family: 'Inter', sans-serif;
    `;
    detectedCard.insertBefore(badge, detectedCard.firstChild);
  }

  /* ── Activate default instruction tab based on OS ── */
  const defaultTab = document.querySelector(`.tab-btn[data-tab="tab-${detectedOS}"]`);
  if (defaultTab) {
    tabBtns.forEach(b => b.classList.remove('active'));
    tabPanels.forEach(p => p.classList.remove('active'));
    defaultTab.classList.add('active');
    document.getElementById(`tab-${detectedOS}`).classList.add('active');
  }

});
