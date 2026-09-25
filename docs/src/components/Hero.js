import Link from '@docusaurus/Link'
import useBaseUrl from '@docusaurus/useBaseUrl'
import styles from './Hero.module.css'

export default function Hero() {
  const logo = useBaseUrl('/img/card.png')
  return (
    <header className={styles.hero}>
      <img className={styles.logo} src={logo} alt="CrabIndex" />
      <div className={styles.text}>
        <p className={styles.tagline}>
          Агрегатор торрент-трекеров для{' '}
          <a href="https://t.me/prisma_party">Prisma</a>, Lampa, Sonarr и Prowlarr. Собственная база FileDB и API,
          совместимый с Jackett, Torznab и Prowlarr. Один бинарник на Rust.
        </p>
        <div className={styles.buttons}>
          <Link className="button button--primary button--lg" to="/quickstart/">
            Быстрый старт
          </Link>
          <Link className="button button--secondary button--lg" to="/api-reference/overview/">
            Справочник API
          </Link>
          <a className="button button--outline button--secondary button--lg" href="/" target="_self">
            Открыть веб-интерфейс
          </a>
        </div>
      </div>
    </header>
  )
}
