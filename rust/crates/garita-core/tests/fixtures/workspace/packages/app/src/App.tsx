import { greet } from "@fixture/lib";

export function App(props: { name: string }) {
  return <main>{greet(props.name)}</main>;
}
